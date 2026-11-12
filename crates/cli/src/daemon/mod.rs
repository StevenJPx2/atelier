//! `atelier daemon`: one poll thread per watched project, one writer for
//! `state.json`, and a lock so only one daemon runs.

mod lock;
mod poll;
mod writer;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use color_eyre::eyre::Result;

use crate::log::log;
use crate::paths::Paths;
use crate::projects;

/// How often the daemon rereads `projects.toml` for projects added or
/// removed; reading a small local file costs nothing.
const SUPERVISE: Duration = Duration::from_secs(5);

struct Running {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

/// Runs until the process is stopped. State is written before every action,
/// so stopping at any moment loses nothing.
///
/// # Errors
///
/// Another daemon holds the lock, or the state cannot be read.
pub fn run(paths: &Paths) -> Result<()> {
    let _lock = lock::take(&paths.lock())?;
    let writer = writer::start(paths.state_file())?;
    let mut running: BTreeMap<String, Running> = BTreeMap::new();

    log(
        "atelier",
        &format!("daemon {} started", env!("CARGO_PKG_VERSION")),
    );

    loop {
        match projects::read(&paths.projects()) {
            Ok(entries) => {
                for entry in &entries {
                    if !running.contains_key(&entry.repo) {
                        let stop = Arc::new(AtomicBool::new(false));
                        let handle = poll::spawn(
                            entry.clone(),
                            paths.clone(),
                            writer.clone(),
                            Arc::clone(&stop),
                        )?;
                        running.insert(entry.repo.clone(), Running { stop, handle });
                    }
                }

                for (repo, project) in &running {
                    if !entries.iter().any(|entry| &entry.repo == repo) {
                        project.stop.store(true, Ordering::Relaxed);
                    }
                }
            }
            Err(error) => log("atelier", &format!("{error:#}")),
        }

        running.retain(|repo, project| {
            let finished = project.handle.is_finished();

            if finished {
                log(repo, "stopped watching");
            }

            !finished
        });

        thread::sleep(SUPERVISE);
    }
}
