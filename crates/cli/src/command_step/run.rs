//! Running a step's command in the background, so a poll never waits on
//! it, and reading how it ended on a later poll.
//!
//! Each start runs in its own folder, which holds what a later poll reads:
//! `started`, the time it started; `group`, its process group; `output`,
//! what it printed; and `exit`, its exit status once it ends. Its process
//! group is not the daemon's, so a daemon restarted while it runs leaves it
//! running.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use atelier_core::{Outcome, PluginError, Settings};

use super::DEFAULT_TIMEOUT_SECS;

/// Starts the command in the background and returns: `$1` is the command
/// and `$2` the worktree. The status file is renamed into place, so a
/// poll never reads it half written.
const SCRIPT: &str = r#"date +%s > started
echo $$ > group
here=$PWD
( cd "$2" && sh -c "$1"; echo $? > "$here/exit.tmp"; mv "$here/exit.tmp" "$here/exit" ) \
  < /dev/null > output 2>&1 &
"#;

/// The most of a failed command's output its outcome tells: its end.
const TAIL_BYTES: u64 = 4096;

/// How many of those last lines the outcome keeps.
const TAIL_LINES: usize = 20;

/// Starts the step's command in `worktree`, keeping what later polls read
/// in `folder`, emptied first, so a round started again starts clean.
/// `ATELIER_BASE` names `origin/<base>`, as for the gates.
///
/// # Errors
///
/// The folder cannot be made, or `sh` cannot start; both transient.
pub fn start(
    folder: &Path,
    worktree: &Path,
    base: &str,
    settings: &Settings,
) -> Result<(), PluginError> {
    let run = settings.text("", "run").ok().flatten().unwrap_or_default();
    let failed = |what: &str, error: std::io::Error| {
        PluginError::transient(format!("{what} {}: {error}", folder.display()))
    };

    if folder.exists() {
        fs::remove_dir_all(folder).map_err(|error| failed("emptying", error))?;
    }

    fs::create_dir_all(folder).map_err(|error| failed("making", error))?;

    let status = Command::new("sh")
        .args(["-c", SCRIPT, "atelier-command", run])
        .arg(worktree)
        .current_dir(folder)
        .env("ATELIER_BASE", format!("origin/{base}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .status()
        .map_err(|error| failed("starting a command in", error))?;

    if status.success() {
        Ok(())
    } else {
        Err(PluginError::transient(format!(
            "the command in {} could not start",
            folder.display()
        )))
    }
}

/// How the command started in `folder` ended, if it has: `passed` on
/// exit 0, `failed` on any other exit, past its time limit, which kills
/// it with every process it started, or when it stopped without an exit
/// status, as on a restart of the computer.
///
/// # Errors
///
/// No command was started in `folder`.
pub fn observe(folder: &Path, settings: &Settings) -> Result<Option<Outcome>, PluginError> {
    if let Some(outcome) = ended(folder) {
        return Ok(Some(outcome));
    }

    let read = |name: &str| {
        fs::read_to_string(folder.join(name))
            .ok()
            .and_then(|text| text.trim().parse::<u64>().ok())
    };
    let (Some(started), Some(group)) = (read("started"), read("group")) else {
        return Err(PluginError::permanent(format!(
            "no command was started in {}",
            folder.display()
        )));
    };

    if !alive(group) {
        return Ok(Some(ended(folder).unwrap_or_else(|| {
            failed("it stopped before it exited, as on a restart of the computer".into())
        })));
    }

    let timeout = settings
        .number("", "timeout")
        .ok()
        .flatten()
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    let limit = u64::try_from(timeout).unwrap_or(u64::MAX);

    if now().saturating_sub(started) >= limit {
        kill(group);
        return Ok(Some(failed(format!(
            "it ran past its {timeout}-second limit{}",
            tail(folder)
        ))));
    }

    Ok(None)
}

/// The outcome its exit status gives, once it has one.
fn ended(folder: &Path) -> Option<Outcome> {
    let status = fs::read_to_string(folder.join("exit")).ok()?;

    match status.trim() {
        "0" => Some(Outcome {
            name: "passed".into(),
            why: "it exited 0".into(),
        }),
        code => Some(failed(format!("it exited {code}{}", tail(folder)))),
    }
}

fn failed(why: String) -> Outcome {
    Outcome {
        name: "failed".into(),
        why,
    }
}

/// The last lines it printed, after a colon, or nothing when it printed
/// nothing.
fn tail(folder: &Path) -> String {
    let Ok(mut file) = File::open(folder.join("output")) else {
        return String::new();
    };
    let length = file.metadata().map_or(0, |metadata| metadata.len());
    let mut bytes = Vec::new();

    if file
        .seek(SeekFrom::Start(length.saturating_sub(TAIL_BYTES)))
        .is_err()
        || file.read_to_end(&mut bytes).is_err()
    {
        return String::new();
    }

    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let last = lines
        .get(lines.len().saturating_sub(TAIL_LINES)..)
        .unwrap_or_default();

    if last.is_empty() {
        String::new()
    } else {
        format!(":\n{}", last.join("\n"))
    }
}

/// Whether any process of the group is left.
fn alive(group: u64) -> bool {
    signal("-0", group)
}

/// Kills the group's every process.
fn kill(group: u64) {
    signal("-KILL", group);
}

fn signal(signal: &str, group: u64) -> bool {
    signal_command(signal, group)
        .is_some_and(|mut command| command.status().is_ok_and(|status| status.success()))
}

/// Prepares an explicit group target without starting a child process.
fn signal_command(signal: &str, group: u64) -> Option<Command> {
    // Targets 0 and 1 would address our group or every permitted process,
    // and `kill` truncates a larger number into its pid range, so 2^32 + 1
    // would reach every process too.
    if !i32::try_from(group).is_ok_and(|group| group > 1) {
        return None;
    }

    let mut command = Command::new("kill");
    command
        .args([signal, "--", &format!("-{group}")])
        .stderr(Stdio::null());

    Some(command)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}
#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::{Duration, Instant};

    use atelier_core::Setting;

    use super::*;

    /// A worktree and a step's folder, removed when the test ends.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("atelier-command-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("worktree")).unwrap();
            Self(root.canonicalize().unwrap())
        }

        fn folder(&self) -> std::path::PathBuf {
            self.0.join("step")
        }

        fn worktree(&self) -> std::path::PathBuf {
            self.0.join("worktree")
        }

        fn start(&self, run: &str, timeout: Option<i64>) -> Settings {
            let mut settings = Settings::default().with("run", Setting::Text(run.into()));

            if let Some(secs) = timeout {
                settings = settings.with("timeout", Setting::Number(secs));
            }

            start(&self.folder(), &self.worktree(), "v2", &settings).unwrap();
            settings
        }

        /// The outcome, once there is one, within ten seconds.
        fn outcome(&self, settings: &Settings) -> Outcome {
            let begun = Instant::now();

            loop {
                if let Some(outcome) = observe(&self.folder(), settings).unwrap() {
                    return outcome;
                }

                assert!(begun.elapsed() < Duration::from_secs(10), "no outcome");
                thread::sleep(Duration::from_millis(50));
            }
        }

        fn group(&self) -> u64 {
            fs::read_to_string(self.folder().join("group"))
                .unwrap()
                .trim()
                .parse()
                .unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn signal_commands_reject_broad_targets_without_execution() {
        use std::ffi::OsStr;

        // Inspect preparation only: even a missing guard must never run kill.
        for signal in ["-0", "-KILL"] {
            for group in [0, 1, 1 << 32, (1 << 32) + 1, u64::MAX] {
                assert!(
                    signal_command(signal, group).is_none(),
                    "prepared {signal} for forbidden group {group}"
                );
            }

            let command = signal_command(signal, 2).expect("group 2 is an explicit target");
            let args: Vec<&OsStr> = command.get_args().collect();

            assert_eq!(command.get_program(), OsStr::new("kill"));
            assert_eq!(
                args,
                [OsStr::new(signal), OsStr::new("--"), OsStr::new("-2")]
            );
        }
    }

    #[test]
    fn a_command_that_exits_0_passes_and_runs_in_the_worktree_with_its_base() {
        let scratch = Scratch::new("passes");

        let settings = scratch.start("pwd > here; printf %s \"$ATELIER_BASE\" > base", None);
        let started = Instant::now();
        let outcome = scratch.outcome(&settings);

        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(
            outcome,
            Outcome {
                name: "passed".into(),
                why: "it exited 0".into(),
            }
        );
        assert_eq!(
            fs::read_to_string(scratch.worktree().join("here"))
                .unwrap()
                .trim(),
            scratch.worktree().to_string_lossy()
        );
        assert_eq!(
            fs::read_to_string(scratch.worktree().join("base")).unwrap(),
            "origin/v2"
        );
    }

    #[test]
    fn a_failing_command_fails_with_the_end_of_what_it_printed() {
        let scratch = Scratch::new("fails");

        let settings = scratch.start("for i in $(seq 1 30); do echo line $i; done; exit 3", None);
        let outcome = scratch.outcome(&settings);
        let lines: Vec<String> = (11..=30).map(|line| format!("line {line}")).collect();

        assert_eq!(outcome.name, "failed");
        assert_eq!(outcome.why, format!("it exited 3:\n{}", lines.join("\n")));
    }

    #[test]
    fn a_command_past_its_limit_fails_and_is_killed_with_its_children() {
        let scratch = Scratch::new("limit");

        let settings = scratch.start("echo waiting; sleep 30 & sleep 30", Some(1));
        let group = scratch.group();
        let begun = Instant::now();
        let outcome = scratch.outcome(&settings);

        assert!(
            begun.elapsed() < Duration::from_secs(4),
            "{:?}",
            begun.elapsed()
        );
        assert_eq!(outcome.name, "failed");
        assert_eq!(outcome.why, "it ran past its 1-second limit:\nwaiting");
        thread::sleep(Duration::from_millis(200));
        assert!(!alive(group));
    }

    #[test]
    fn a_command_still_running_has_no_outcome_yet() {
        let scratch = Scratch::new("running");

        let settings = scratch.start("sleep 30", None);

        assert_eq!(observe(&scratch.folder(), &settings).unwrap(), None);
        kill(scratch.group());
    }

    #[test]
    fn a_command_that_stopped_without_an_exit_status_fails() {
        let scratch = Scratch::new("stopped");

        let settings = scratch.start("sleep 30", None);
        kill(scratch.group());
        let outcome = scratch.outcome(&settings);

        assert_eq!(outcome.name, "failed");
        assert_eq!(
            outcome.why,
            "it stopped before it exited, as on a restart of the computer"
        );
    }

    #[test]
    fn starting_again_starts_clean_and_no_start_is_refused() {
        let scratch = Scratch::new("again");
        let never = observe(&scratch.folder(), &Settings::default()).unwrap_err();

        let first = scratch.start("exit 1", None);
        assert_eq!(scratch.outcome(&first).name, "failed");
        let again = scratch.start("sleep 30", None);

        assert_eq!(observe(&scratch.folder(), &again).unwrap(), None);
        assert!(never.is_permanent());
        assert!(
            never.message.starts_with("no command was started in"),
            "{never}"
        );
        kill(scratch.group());
    }
}
