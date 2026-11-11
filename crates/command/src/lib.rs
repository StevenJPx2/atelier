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

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use atelier_core::PluginError;

/// How often a running command is checked.
const TICK: Duration = Duration::from_millis(20);

/// The most output kept from one stream.
const MAX_OUTPUT: u64 = 1 << 20;

/// What a finished command printed, and whether it succeeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `command` with no input, in its own process group, keeping at most
/// 1 MiB of each output.
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
                let _ = Command::new("kill")
                    .args(["-KILL", "--", &format!("-{}", child.id())])
                    .status();
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

fn reader(stream: impl Read + Send + 'static) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut text = String::new();
        let _ = stream.take(MAX_OUTPUT).read_to_string(&mut text);
        text
    })
}

fn collect(handle: thread::JoinHandle<String>) -> String {
    handle.join().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_missing_tool_fails_transiently() {
        let error = run(Command::new("/no/such/tool"), Duration::from_secs(1)).unwrap_err();

        assert!(!error.is_permanent());
        assert!(
            error.message.contains("/no/such/tool could not start"),
            "{error}"
        );
    }
}
