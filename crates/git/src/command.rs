//! Running `git` with a deadline and bounded output.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use atelier_core::PluginError;

/// The longest any one git command may run; a fetch over a slow link
/// finishes well within it.
const DEADLINE: Duration = Duration::from_secs(120);

/// How often a running command is checked.
const TICK: Duration = Duration::from_millis(20);

/// The most output kept from one stream; git's answers here are a line.
const MAX_OUTPUT: u64 = 1 << 20;

/// What a finished git command printed, and whether it succeeded.
pub struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `git` in `dir` with `args`, never prompting for credentials.
///
/// # Errors
///
/// Git cannot start, or runs past the deadline and is killed; both
/// transient, so the next poll tries again.
pub fn git(dir: &Path, args: &[&str]) -> Result<Output, PluginError> {
    run(dir, args, DEADLINE)
}

fn run(dir: &Path, args: &[&str], deadline: Duration) -> Result<Output, PluginError> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|error| PluginError::transient(format!("git could not start: {error}")))?;

    let stdout = child.stdout.take().map(reader);
    let stderr = child.stderr.take().map(reader);
    let started = Instant::now();

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() >= deadline => {
                // The group too: a child git started, such as ssh, would
                // otherwise hold the output open past the deadline.
                let _ = Command::new("kill")
                    .args(["-KILL", "--", &format!("-{}", child.id())])
                    .status();
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!(
                    "git {} ran past {}s",
                    args.join(" "),
                    DEADLINE.as_secs()
                ));
            }
            Ok(None) => thread::sleep(TICK),
            Err(error) => break Err(format!("git {}: {error}", args.join(" "))),
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

/// Fails with git's own message when the command did not succeed.
///
/// # Errors
///
/// The command failed; a failure to reach the remote is transient,
/// anything else permanent.
pub fn checked(dir: &Path, args: &[&str]) -> Result<String, PluginError> {
    let output = git(dir, args)?;

    if output.success {
        return Ok(output.stdout);
    }

    let message = format!("git {}: {}", args.join(" "), output.stderr.trim());
    let remote = args
        .first()
        .is_some_and(|command| ["fetch", "ls-remote"].contains(command));

    if remote {
        Err(PluginError::transient(message))
    } else {
        Err(PluginError::permanent(message))
    }
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
    fn a_command_past_its_deadline_is_killed_and_retried_later() {
        let started = Instant::now();
        let nap = ["-c", "alias.nap=!sleep 5", "nap"];

        let error = run(Path::new("."), &nap, Duration::from_millis(200))
            .err()
            .unwrap();

        assert!(!error.is_permanent());
        assert!(error.message.contains("ran past"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn a_failed_command_is_permanent_unless_it_reaches_the_remote() {
        let local = checked(Path::new("."), &["rev-parse", "--verify", "no-such-ref"]);
        let remote = checked(Path::new("."), &["ls-remote", "/no/such/remote"]);

        assert!(local.unwrap_err().is_permanent());
        assert!(!remote.unwrap_err().is_permanent());
    }
}
