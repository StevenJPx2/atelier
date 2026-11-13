//! Worktrees and branches through the `git` command, behind core's
//! `Worktrees` role, and the `git.worktree` kind of step.
//!
//! ```no_run
//! use std::path::Path;
//!
//! use atelier_core::{Repo, WorktreeRequest, Worktrees};
//! use atelier_git::Git;
//!
//! let git = Git::new("/Users/me/.local/share/atelier/worktrees");
//! let repo = Repo("StevenJPx2/chauffeur".into());
//!
//! let tree = git.create(&WorktreeRequest {
//!     repo: &repo,
//!     local: Path::new("/Users/me/Documents/Projects/chauffeur"),
//!     name: "issue-12",
//!     branch: "issue-12-fix-the-dead-link",
//!     base: "v2",
//! })?;
//! // → /Users/me/.local/share/atelier/worktrees/StevenJPx2/chauffeur/issue-12
//! # Ok::<(), atelier_core::PluginError>(())
//! ```

mod command;
mod folder;
mod repository;
mod steps;
mod tree;
mod worktree;

pub use repository::{BranchTree, Repository};
pub use tree::{Staged, WorkingTree};
pub use worktree::Git;
