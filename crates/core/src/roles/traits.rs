use std::path::{Path, PathBuf};

use super::{Filter, Issue, Listing, PluginError, Repo, Session};
use crate::{Brief, Mark, PullRequest};

/// Where issues live: lists, reads, marks, comments on and closes them.
pub trait Tracker {
    /// Open issues carrying `mark` that match `filter`, or `Unchanged`
    /// when `etag` still holds.
    ///
    /// # Errors
    ///
    /// The tracker's failure, such as a rate limit.
    fn issues(
        &self,
        repo: &Repo,
        mark: Mark,
        filter: &Filter,
        etag: Option<&str>,
    ) -> Result<Listing, PluginError>;

    /// # Errors
    ///
    /// The tracker's failure.
    fn issue(&self, repo: &Repo, id: &str) -> Result<Issue, PluginError>;

    /// Adds a mark; adding one already there succeeds.
    ///
    /// # Errors
    ///
    /// The tracker's failure.
    fn mark(&self, repo: &Repo, id: &str, mark: Mark) -> Result<(), PluginError>;

    /// Removes a mark; removing one that is not there succeeds.
    ///
    /// # Errors
    ///
    /// The tracker's failure.
    fn unmark(&self, repo: &Repo, id: &str, mark: Mark) -> Result<(), PluginError>;

    /// # Errors
    ///
    /// The tracker's failure.
    fn comment(&self, repo: &Repo, id: &str, text: &str) -> Result<(), PluginError>;

    /// Closes the issue; closing a closed one succeeds.
    ///
    /// # Errors
    ///
    /// The tracker's failure.
    fn close(&self, repo: &Repo, id: &str) -> Result<(), PluginError>;

    /// How the builder refers to the issue in a pull request, such as `#12`.
    fn reference(&self, issue: &Issue) -> String;

    /// The brief's part on reading the issue and marking it blocked.
    fn brief(&self, repo: &Repo, issue: &Issue) -> String;
}

/// Where the code is reviewed: pull requests and their checks.
pub trait Forge {
    /// The pull request on `branch`, newest first, if any.
    ///
    /// # Errors
    ///
    /// The forge's failure.
    fn pull_request(&self, repo: &Repo, branch: &str) -> Result<Option<PullRequest>, PluginError>;

    /// The brief's part on opening, watching and readying the pull request.
    fn brief(&self, repo: &Repo, base: &str, branch: &str) -> String;

    /// Commands the builder may never run, as shell patterns.
    fn denied(&self) -> Vec<String>;
}

/// The coding agent builders run in.
pub trait Agent {
    /// Starts a session in `worktree` working on `brief`, returning once the
    /// agent has accepted it.
    ///
    /// # Errors
    ///
    /// The agent's failure.
    fn start(&self, worktree: &Path, brief: &Brief) -> Result<Session, PluginError>;

    /// Stops the session's turn; interrupting an idle session succeeds.
    ///
    /// # Errors
    ///
    /// The agent's failure.
    fn interrupt(&self, session: &Session) -> Result<(), PluginError>;
}

/// A worktree an issue's builder works in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRequest<'a> {
    pub repo: &'a Repo,
    pub checkout: &'a Path,
    pub name: &'a str,
    pub branch: &'a str,
    pub base: &'a str,
}

/// Worktrees and branches, behind a trait so core's tests stand them in.
pub trait Worktrees {
    /// The worktree on the request's branch, made from the base or reused.
    ///
    /// # Errors
    ///
    /// Git's failure.
    fn create(&self, request: &WorktreeRequest<'_>) -> Result<PathBuf, PluginError>;

    /// Removes the worktree; removing one that is gone succeeds.
    ///
    /// # Errors
    ///
    /// Git's failure.
    fn remove(&self, checkout: &Path, worktree: &Path) -> Result<(), PluginError>;
}
