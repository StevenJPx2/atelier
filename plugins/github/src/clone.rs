//! Where atelier clones a repository from.

use std::process::Command;

use atelier_core::{PluginError, Repo};

use crate::GitHub;
use crate::api::DEADLINE;

impl GitHub {
    /// The URL to clone `repo` from, over the protocol `gh` uses for
    /// github.com, as `gh repo clone` does: HTTPS unless it is set to SSH.
    /// Either way `git` reaches GitHub with the user's own sign-in.
    ///
    /// # Errors
    ///
    /// `gh` cannot start, or runs past its deadline; both transient.
    pub fn clone_url(&self, repo: &Repo) -> Result<String, PluginError> {
        let mut command = Command::new(&self.program);
        command
            .args(["config", "get", "git_protocol", "-h", "github.com"])
            .env("GH_PROMPT_DISABLED", "1");

        let output = atelier_command::run(command, DEADLINE)?;
        let name = &repo.0;

        if output.success && output.stdout.trim() == "ssh" {
            Ok(format!("git@github.com:{name}.git"))
        } else {
            Ok(format!("https://github.com/{name}.git"))
        }
    }
}
