//! What atelier reads of a project's local repository and its `origin`.

use std::path::{Path, PathBuf};

use atelier_core::PluginError;

use crate::command::{checked, git};

/// A project's local repository, bare or a checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    path: PathBuf,
}

impl Repository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The URL of `origin` as configured, before git's own `insteadOf`
    /// rewriting, so it names the forge the user chose.
    ///
    /// # Errors
    ///
    /// The folder is not a repository, or has no `origin`.
    pub fn origin(&self) -> Result<String, PluginError> {
        let url = checked(&self.path, &["config", "--get", "remote.origin.url"])?;

        Ok(url.trim().to_owned())
    }

    /// The branch `origin`'s `HEAD` points at, asked of `origin` itself.
    ///
    /// # Errors
    ///
    /// `origin` cannot be reached, or names no default branch.
    pub fn default_branch(&self) -> Result<String, PluginError> {
        let heads = checked(&self.path, &["ls-remote", "--symref", "origin", "HEAD"])?;

        heads
            .lines()
            .find_map(|line| line.strip_prefix("ref: refs/heads/")?.split_once('\t'))
            .map(|(branch, _)| branch.to_owned())
            .ok_or_else(|| PluginError::permanent("origin names no default branch"))
    }

    /// Fetches `branch` from `origin`, so `origin/<branch>` is current.
    ///
    /// # Errors
    ///
    /// `origin` cannot be reached, or has no such branch.
    pub fn fetch(&self, branch: &str) -> Result<(), PluginError> {
        checked(&self.path, &["fetch", "--quiet", "origin", branch])?;
        Ok(())
    }

    /// The file at `path` on `origin/<branch>`, as last fetched; none when
    /// the branch has no such file.
    ///
    /// # Errors
    ///
    /// Git cannot run, or `origin/<branch>` is unknown.
    pub fn show(&self, branch: &str, path: &str) -> Result<Option<String>, PluginError> {
        let remote = format!("origin/{branch}");
        checked(&self.path, &["rev-parse", "--verify", "--quiet", &remote])?;

        let output = git(&self.path, &["show", &format!("{remote}:{path}")])?;

        Ok(output.success.then_some(output.stdout))
    }
}
