//! atelier's core: the issue machine, the roles plugins fill, and which
//! gates a change needs. It does no I/O, has no dependencies, and knows no
//! language, tracker, forge or agent.

mod brief;
mod checks;
mod machine;
mod plugin;
mod poll;
mod roles;
mod seen;
mod settings;

pub use brief::Brief;
pub use checks::{
    Checks, Fix, Gate, MemoryTree, Override, Overrides, Planned, Root, Tree, fixes, gates,
};
pub use machine::{Action, Event, Mark, Phase, Pipeline, PipelineError, Reason, step};
pub use plugin::{MakeChecks, MakeTracker, MissingPlugin, On, Plugin, Registry, Role};
pub use poll::{
    Observation, Observed, Plugins, Project, Record, Report, Source, State, Step, act, observe,
};
pub use roles::{
    Agent, CiFile, Failure, Filter, Forge, Issue, IssueKey, Listing, PluginError, Repo, Session,
    Tracker, WorktreeRequest, Worktrees,
};
pub use seen::{Marks, PullRequest, Rollup, Seen, StageOutcome, events};
pub use settings::{Setting, Settings, SettingsError};
