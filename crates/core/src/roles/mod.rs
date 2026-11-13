//! The roles a plugin fills, each a trait, and the values that cross them.

mod error;
mod issue;
mod step;
mod traits;

pub use error::{Failure, PluginError};
pub use issue::{Filter, Issue, IssueKey, Listing, Repo, Session};
pub use step::{Kind, Observed, Outcome, Outcomes, RunContext, Started, Step};
pub use traits::{Agent, CiFile, Forge, Tracker, WorktreeRequest, Worktrees};
