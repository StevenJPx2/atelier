//! Each issue's worktree, on its branch, under one root.

use std::path::{Path, PathBuf};

use atelier_core::{PluginError, Repo, WorktreeRequest, Worktrees};

use crate::command::{checked, git};

/// Worktrees under `root`, as `<root>/<owner>/<repo>/<name>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Git {
    root: PathBuf,
}

impl Git {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path(&self, repo: &Repo, name: &str) -> Result<PathBuf, PluginError> {
        let mut parts = repo.0.split('/');

        let (Some(owner), Some(project), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(PluginError::permanent(format!(
                "{} is not owner/name",
                repo.0
            )));
        };

        for part in [owner, project, name] {
            if !safe(part) {
                return Err(PluginError::permanent(format!(
                    "{part:?} cannot name a folder"
                )));
            }
        }

        Ok(self.root.join(owner).join(project).join(name))
    }
}

/// Letters, digits, `-`, `_` and `.`, and not only dots.
fn safe(part: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || "-_.".contains(c);

    !part.is_empty() && part.chars().all(allowed) && part.chars().any(|c| c != '.')
}

impl Worktrees for Git {
    /// Reuses the worktree when it is already on the branch. Otherwise makes
    /// it from the branch, local or on `origin`, or a new branch from
    /// `origin/<base>`. A local branch is never reset.
    fn create(&self, request: &WorktreeRequest<'_>) -> Result<PathBuf, PluginError> {
        let path = self.path(request.repo, request.name)?;
        let branch = request.branch;

        if path.exists() {
            let head = checked(&path, &["rev-parse", "--abbrev-ref", "HEAD"])?;

            return if head.trim() == branch {
                Ok(path)
            } else {
                Err(PluginError::permanent(format!(
                    "{} is on {}, not {branch}",
                    path.display(),
                    head.trim()
                )))
            };
        }

        let checkout = request.checkout;
        let target = path.to_string_lossy();
        let local = format!("refs/heads/{branch}");
        let tracking = format!("origin/{branch}");
        let base = format!("origin/{}", request.base);

        checked(checkout, &["fetch", "origin", request.base])?;

        if git(checkout, &["show-ref", "--verify", "--quiet", &local])?.success {
            checked(checkout, &["worktree", "add", &target, branch])?;
        } else if remote(checkout, branch)? {
            checked(checkout, &["fetch", "origin", branch])?;
            checked(
                checkout,
                &[
                    "worktree", "add", "--track", "-b", branch, &target, &tracking,
                ],
            )?;
        } else {
            checked(
                checkout,
                &[
                    "worktree",
                    "add",
                    "--no-track",
                    "-b",
                    branch,
                    &target,
                    &base,
                ],
            )?;
        }

        Ok(path)
    }

    /// Removes the worktree, even with changes the builder left, and keeps
    /// its branch. A worktree already gone is pruned from git's list.
    fn remove(&self, checkout: &Path, worktree: &Path) -> Result<(), PluginError> {
        if worktree.exists() {
            let target = worktree.to_string_lossy();
            checked(checkout, &["worktree", "remove", "--force", &target])?;
        } else {
            checked(checkout, &["worktree", "prune"])?;
        }

        Ok(())
    }
}

fn remote(checkout: &Path, branch: &str) -> Result<bool, PluginError> {
    let heads = checked(checkout, &["ls-remote", "--heads", "origin", branch])?;

    Ok(!heads.trim().is_empty())
}
