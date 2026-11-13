use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use atelier_core::{IssueKey, Request};
use color_eyre::eyre::{Result, WrapErr, bail};

use super::file::File;

/// A project's queue has at most this many requests, including writers
/// still writing one. A full queue refuses another request, never waits.
const SLOTS: usize = 128;

/// A request holds at most 8 KiB, including its reason.
const MAX_BYTES: u64 = 8192;

/// A request read by the worker, kept until its effects have been saved.
pub struct Queued {
    pub key: IssueKey,
    pub request: Request,
    path: PathBuf,
}

/// Writes a request atomically: a slot's temporary file reserves it
/// before it is written and renamed. A reader never sees half a request.
///
/// # Errors
///
/// The queue is full, the request is too large, or a file cannot be
/// written.
pub fn submit(folder: &Path, key: &IssueKey, request: &Request) -> Result<()> {
    let bytes = serde_json::to_vec(&File::new(key, request))?;

    if u64::try_from(bytes.len())? > MAX_BYTES {
        bail!("the request is too large; at most {MAX_BYTES} bytes");
    }

    fs::create_dir_all(folder)?;

    for slot in 0..SLOTS {
        let path = folder.join(format!("{slot}.json"));

        if path.exists() {
            continue;
        }

        let temporary = folder.join(format!("{slot}.tmp"));
        let mut writer = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(writer) => writer,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).wrap_err("reserving a request slot"),
        };
        let result = (|| {
            // A prior writer may have renamed its file after our first
            // check, but before this writer reserved the same slot.
            if path.exists() {
                return Ok(false);
            }

            writer.write_all(&bytes)?;
            writer.sync_all()?;
            fs::rename(&temporary, &path)?;
            Ok::<_, std::io::Error>(true)
        })();
        if release(&temporary, result)? {
            return Ok(());
        }
    }

    bail!("the project's request queue is full ({SLOTS} requests)")
}

/// Releases a writer's temporary reservation after its write.
fn release(temporary: &Path, result: std::io::Result<bool>) -> Result<bool> {
    if !matches!(result, Ok(true)) {
        let _ = fs::remove_file(temporary);
    }

    result.wrap_err("writing a request")
}

/// Reads a project's ready requests, at most `SLOTS`, with no wait.
///
/// # Errors
///
/// A ready file cannot be read, is too large, or is not a request.
pub fn read(folder: &Path) -> Result<Vec<Queued>> {
    let mut queued = Vec::new();

    for slot in 0..SLOTS {
        let path = folder.join(format!("{slot}.json"));
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).wrap_err_with(|| format!("reading {}", path.display()));
            }
        };
        let mut bytes = Vec::new();
        file.take(MAX_BYTES.saturating_add(1))
            .read_to_end(&mut bytes)?;

        if u64::try_from(bytes.len())? > MAX_BYTES {
            bail!("{} exceeds {MAX_BYTES} bytes", path.display());
        }

        let file: File = serde_json::from_slice(&bytes)
            .wrap_err_with(|| format!("reading {}", path.display()))?;
        let (key, request) = file.into_request();

        queued.push(Queued { key, request, path });
    }

    Ok(queued)
}

/// Removes the requests whose effects the worker has saved. Replaying a
/// request after a crash before this acknowledgement is harmless.
///
/// # Errors
///
/// A file cannot be removed.
pub fn acknowledge(queued: &[Queued]) -> Result<()> {
    for queued in queued {
        match fs::remove_file(&queued.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).wrap_err("acknowledging a request"),
        }
    }

    Ok(())
}
#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use atelier_core::{Command, Outcome};

    use super::*;

    static NEXT: AtomicU32 = AtomicU32::new(0);

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("atelier-requests-{}-{id}", std::process::id()));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn key() -> IssueKey {
        IssueKey {
            tracker: "github".into(),
            id: "40".into(),
        }
    }

    fn finish(why: &str) -> Request {
        Request {
            generation: 2,
            command: Command::Finish {
                step: "audit".into(),
                outcome: Outcome {
                    name: "clean".into(),
                    why: why.into(),
                },
                session: "ses_audit".into(),
                failed: 1,
            },
        }
    }

    #[test]
    fn requests_roundtrip_and_acknowledgement_is_idempotent() {
        let scratch = Scratch::new();
        let finish = finish("no advisories");
        let stop = Request {
            generation: 2,
            command: Command::Stop,
        };

        submit(&scratch.0, &key(), &finish).unwrap();
        submit(&scratch.0, &key(), &stop).unwrap();
        let queued = read(&scratch.0).unwrap();
        acknowledge(&queued).unwrap();
        acknowledge(&queued).unwrap();

        assert_eq!(queued.len(), 2);
        assert_eq!(queued[0].key, key());
        assert_eq!(queued[0].request, finish);
        assert_eq!(queued[1].request, stop);
        assert_eq!(read(&scratch.0).unwrap().len(), 0);
    }

    #[test]
    fn concurrent_writers_keep_every_request() {
        let scratch = Scratch::new();
        let together = std::sync::Barrier::new(64);
        std::thread::scope(|scope| {
            let writers: Vec<_> = (0..64)
                .map(|id| {
                    let root = &scratch.0;
                    let together = &together;
                    scope.spawn(move || {
                        together.wait();
                        submit(root, &key(), &finish(&format!("checked {id}")))
                    })
                })
                .collect();

            for writer in writers {
                writer.join().unwrap().unwrap();
            }
        });

        let queued = read(&scratch.0).unwrap();
        let mut reasons: Vec<_> = queued
            .iter()
            .filter_map(|queued| match &queued.request.command {
                Command::Finish { outcome, .. } => Some(outcome.why.clone()),
                Command::Stop => None,
            })
            .collect();
        reasons.sort();

        let mut expected = (0..64)
            .map(|id| format!("checked {id}"))
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(reasons, expected);
    }

    #[test]
    fn a_successful_rename_does_not_release_the_next_writers_reservation() {
        let scratch = Scratch::new();
        let temporary = scratch.0.join("0.tmp");
        let ready = scratch.0.join("0.json");
        fs::write(&temporary, "first").unwrap();
        fs::rename(&temporary, &ready).unwrap();
        // A second writer checked the ready path before the first rename,
        // then reserved its temporary path after the rename.
        fs::write(&temporary, "second writer owns this").unwrap();

        let written = release(&temporary, Ok(true)).unwrap();

        assert!(written);
        assert_eq!(
            fs::read_to_string(&temporary).unwrap(),
            "second writer owns this"
        );
        assert_eq!(fs::read_to_string(&ready).unwrap(), "first");
    }

    #[test]
    fn temporary_files_are_not_read_and_a_full_queue_refuses_without_waiting() {
        let scratch = Scratch::new();

        for slot in 0..SLOTS {
            fs::write(scratch.0.join(format!("{slot}.tmp")), "half written").unwrap();
        }

        let queued = read(&scratch.0).unwrap();
        let full = submit(&scratch.0, &key(), &finish("clean")).unwrap_err();

        assert_eq!(queued.len(), 0);
        assert!(full.to_string().contains("queue is full"));
    }

    #[test]
    fn oversized_or_malformed_requests_are_refused() {
        let scratch = Scratch::new();
        let huge = finish(&"x".repeat(8193));

        let refused = submit(&scratch.0, &key(), &huge).unwrap_err();
        fs::write(scratch.0.join("0.json"), "not json").unwrap();
        let malformed = read(&scratch.0).err().unwrap();
        fs::write(scratch.0.join("0.json"), "x".repeat(8193)).unwrap();
        let too_big = read(&scratch.0).err().unwrap();

        assert!(refused.to_string().contains("too large"));
        assert!(malformed.to_string().contains("reading"));
        assert!(too_big.to_string().contains("exceeds 8192"));
    }
}
