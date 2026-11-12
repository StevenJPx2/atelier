//! Running `git` with a deadline, never prompting for credentials.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use atelier_command::Output;
use atelier_core::PluginError;

/// The longest any one git command may run; a fetch over a slow link
/// finishes well within it.
const DEADLINE: Duration = Duration::from_secs(120);

/// Runs `git` in `dir` with `args`.
///
/// # Errors
///
/// Git cannot start, or runs past the deadline; both transient.
pub fn git(dir: &Path, args: &[&str]) -> Result<Output, PluginError> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0");

    atelier_command::run(command, DEADLINE)
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

    let said = match output.stderr.trim() {
        "" => "failed, saying nothing",
        said => said,
    };
    let message = format!("git {}: {said}", args.join(" "));
    let remote = args
        .first()
        .is_some_and(|command| ["fetch", "ls-remote"].contains(command));

    if remote {
        Err(PluginError::transient(message))
    } else {
        Err(PluginError::permanent(message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_command_is_permanent_unless_it_reaches_the_remote() {
        let local = checked(Path::new("."), &["rev-parse", "--verify", "no-such-ref"]);
        let remote = checked(Path::new("."), &["ls-remote", "/no/such/remote"]);
        let silent = checked(Path::new("."), &["config", "--get", "atelier.no-such-key"]);

        assert!(local.unwrap_err().is_permanent());
        assert!(!remote.unwrap_err().is_permanent());
        assert_eq!(
            silent.unwrap_err().message,
            "git config --get atelier.no-such-key: failed, saying nothing"
        );
    }
}
