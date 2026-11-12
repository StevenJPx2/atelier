//! Each issue's worktree, on its branch, under one root.

use std::path::{Path, PathBuf};

use atelier_core::{PluginError, Repo, WorktreeRequest, Worktrees};

use crate::command::{checked, git};
use crate::folder;

/// Worktrees under `root`, as `<root>/<owner>/<repo>/<name>`, each with
/// `hooks` as its own hooks folder when set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Git {
    root: PathBuf,
    hooks: Option<PathBuf>,
}

impl Git {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            hooks: None,
        }
    }

    /// Gives each worktree it makes or reuses `hooks` as its hooks folder,
    /// through that worktree's own config, so the repository and its other
    /// worktrees keep theirs.
    #[must_use]
    pub fn with_hooks(self, hooks: impl Into<PathBuf>) -> Self {
        Self {
            hooks: Some(hooks.into()),
            ..self
        }
    }

    fn path(&self, repo: &Repo, name: &str) -> Result<PathBuf, PluginError> {
        let (owner, project) = folder::owner_and_name(repo)?;

        Ok(self
            .root
            .join(owner)
            .join(project)
            .join(folder::checked(name)?))
    }
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

            if head.trim() != branch {
                return Err(PluginError::permanent(format!(
                    "{} is on {}, not {branch}",
                    path.display(),
                    head.trim()
                )));
            }
        } else {
            make(request, &path)?;
        }

        if let Some(hooks) = &self.hooks {
            give_hooks(request.local, &path, hooks)?;
        }

        Ok(path)
    }

    /// Removes the worktree, even with changes the builder left, and keeps
    /// its branch. Git runs inside the worktree, so one made from another
    /// repository, such as your checkout before atelier kept its own clone,
    /// goes too. A worktree already gone is pruned from `local`'s list.
    fn remove(&self, local: &Path, worktree: &Path) -> Result<(), PluginError> {
        if worktree.exists() {
            let target = worktree.to_string_lossy();
            checked(worktree, &["worktree", "remove", "--force", &target])?;
        } else {
            checked(local, &["worktree", "prune"])?;
        }

        Ok(())
    }
}

/// Makes the worktree at `path` from the branch, local or on `origin`, or
/// a new branch from `origin/<base>`.
fn make(request: &WorktreeRequest<'_>, path: &Path) -> Result<(), PluginError> {
    let local = request.local;
    let branch = request.branch;
    let target = path.to_string_lossy();
    let head = format!("refs/heads/{branch}");
    let tracking = format!("origin/{branch}");
    let base = format!("origin/{}", request.base);

    checked(local, &["fetch", "origin", request.base])?;

    if git(local, &["show-ref", "--verify", "--quiet", &head])?.success {
        checked(local, &["worktree", "add", &target, branch])?;
    } else if remote(local, branch)? {
        checked(local, &["fetch", "origin", branch])?;
        checked(
            local,
            &[
                "worktree", "add", "--track", "-b", branch, &target, &tracking,
            ],
        )?;
    } else {
        checked(
            local,
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

    Ok(())
}

/// Gives the worktree `hooks` as its hooks folder, in its own config.
///
/// Per-worktree config needs `extensions.worktreeConfig`. With it, a bare
/// repository's shared `core.bare` would apply to its worktrees too, so it
/// moves to the repository's own `config.worktree` first, and no worktree
/// is ever left reading it.
fn give_hooks(local: &Path, worktree: &Path, hooks: &Path) -> Result<(), PluginError> {
    let enabled = git(local, &["config", "--get", "extensions.worktreeConfig"])?;

    if enabled.stdout.trim() != "true" {
        let bare = checked(local, &["rev-parse", "--is-bare-repository"])?;

        if bare.trim() == "true" {
            let own = local.join("config.worktree");
            let own = own.to_string_lossy();
            checked(local, &["config", "--file", &own, "core.bare", "true"])?;
            checked(local, &["config", "--unset", "core.bare"])?;
        }

        checked(local, &["config", "extensions.worktreeConfig", "true"])?;
    }

    let hooks = hooks.to_string_lossy();
    checked(
        worktree,
        &["config", "--worktree", "core.hooksPath", &hooks],
    )?;

    Ok(())
}

fn remote(local: &Path, branch: &str) -> Result<bool, PluginError> {
    let heads = checked(local, &["ls-remote", "--heads", "origin", branch])?;

    Ok(!heads.trim().is_empty())
}
