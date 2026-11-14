//! Running a tool, such as `git`, `gh` or `opencode`, with a deadline and
//! bounded output, so a hung tool never holds up a poll.
//!
//! ```
//! use std::process::Command;
//! use std::time::Duration;
//!
//! let mut command = Command::new("git");
//! command.arg("--version");
//!
//! let output = atelier_command::run(command, Duration::from_secs(10))?;
//!
//! assert!(output.success);
//! assert!(output.stdout.starts_with("git version"));
//! # Ok::<(), atelier_core::PluginError>(())
//! ```

use std::io::{self, Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use atelier_core::PluginError;

/// How often a running command is checked.
const TICK: Duration = Duration::from_millis(20);

/// The most output kept from one stream: its end, where a tool says why it
/// failed.
const MAX_OUTPUT: usize = 1 << 20;

/// What a finished command printed, and whether it succeeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `command` with no input, in its own process group. Each output is
/// read to its end, so a tool that prints a lot runs on, and its last 1 MiB
/// is kept, with any bytes that are not UTF-8 replaced.
///
/// # Errors
///
/// The tool cannot start, or runs past `deadline` and is killed with every
/// process it started; both transient, so the next poll tries again.
pub fn run(mut command: Command, deadline: Duration) -> Result<Output, PluginError> {
    let program = command.get_program().to_string_lossy().into_owned();

    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|error| PluginError::transient(format!("{program} could not start: {error}")))?;

    let stdout = child.stdout.take().map(reader);
    let stderr = child.stderr.take().map(reader);
    let started = Instant::now();

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() >= deadline => {
                // The group too: a child the tool started, such as ssh, would
                // otherwise hold the output open past the deadline.
                if let Some(mut kill) = signal_group("-KILL", u64::from(child.id())) {
                    let _ = kill.status();
                }
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("{program} ran past {deadline:?}"));
            }
            Ok(None) => thread::sleep(TICK),
            Err(error) => break Err(format!("{program}: {error}")),
        }
    };

    let stdout = stdout.map(collect).unwrap_or_default();
    let stderr = stderr.map(collect).unwrap_or_default();
    let status = status.map_err(PluginError::transient)?;

    Ok(Output {
        success: status.success(),
        stdout,
        stderr,
    })
}

/// Prepares `kill <signal> -- -<group>` for one process group without
/// starting it, or `None` when the target would reach more than that group.
///
/// Every group signal atelier sends is built here, so the guard holds at
/// both the deadline kill above and a background step's cleanup.
///
/// ```
/// let command = atelier_command::signal_group("-KILL", 4242).unwrap();
///
/// assert_eq!(command.get_program(), "kill");
/// assert!(atelier_command::signal_group("-KILL", 1).is_none());
/// ```
#[must_use]
pub fn signal_group(signal: &str, group: u64) -> Option<Command> {
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

/// Reads `stream` to its end, keeping its last `MAX_OUTPUT` bytes. Reading
/// on, rather than closing the stream, keeps a tool that prints past the
/// bound from being stopped by a closed pipe.
fn reader(mut stream: impl Read + Send + 'static) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut tail = Tail(Vec::new());
        let _ = io::copy(&mut stream, &mut tail);

        String::from_utf8_lossy(&tail.0).into_owned()
    })
}

/// A sink that keeps only the last `MAX_OUTPUT` bytes written to it.
struct Tail(Vec<u8>);

impl Write for Tail {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.extend_from_slice(bytes);
        let over = self.0.len().saturating_sub(MAX_OUTPUT);
        self.0.drain(..over);

        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn collect(handle: thread::JoinHandle<String>) -> String {
    handle.join().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_signals_refuse_broad_targets_without_execution() {
        use std::ffi::OsStr;

        let highest = u64::from(i32::MAX.unsigned_abs());
        let refused = [
            0,
            1,
            highest + 1,
            u64::from(u32::MAX),
            1 << 32,
            (1 << 32) + 1,
            u64::MAX,
        ];

        // Inspect preparation only: even a missing guard must never run kill.
        for signal in ["-0", "-KILL"] {
            for group in refused {
                assert!(
                    signal_group(signal, group).is_none(),
                    "prepared {signal} for forbidden group {group}"
                );
            }

            for group in [2, highest] {
                let command = signal_group(signal, group).expect("an explicit target");
                let args: Vec<&OsStr> = command.get_args().collect();
                let target = format!("-{group}");

                assert_eq!(command.get_program(), OsStr::new("kill"));
                assert_eq!(
                    args,
                    [OsStr::new(signal), OsStr::new("--"), OsStr::new(&target)]
                );
            }
        }
    }

    #[test]
    fn a_command_past_its_deadline_is_killed_with_its_children() {
        let started = Instant::now();
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 5 & sleep 5"]);

        let error = run(command, Duration::from_millis(200)).unwrap_err();

        assert!(!error.is_permanent());
        assert!(error.message.contains("ran past"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn output_is_kept_up_to_its_bound() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "head -c 2000000 /dev/zero | tr '\\0' a; echo err >&2; exit 3",
        ]);

        let output = run(command, Duration::from_secs(10)).unwrap();

        assert!(!output.success);
        assert_eq!(output.stdout.len(), 1 << 20);
        assert_eq!(output.stderr, "err\n");
    }

    #[test]
    fn a_tool_writing_past_the_bound_runs_to_its_end_and_its_last_output_is_kept() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "head -c 3000000 /dev/zero | tr '\\0' a; printf '\\377'; echo; echo last",
        ]);

        let output = run(command, Duration::from_secs(10)).unwrap();

        assert!(output.success, "{}", output.stderr);
        assert!(output.stdout.ends_with("a\u{fffd}\nlast\n"));
        assert!(output.stdout.len() <= (1 << 20) + 2);
    }

    #[test]
    fn a_missing_tool_fails_transiently() {
        let error = run(Command::new("/no/such/tool"), Duration::from_secs(1)).unwrap_err();

        assert!(!error.is_permanent());
        assert!(
            error.message.contains("/no/such/tool could not start"),
            "{error}"
        );
    }
}
