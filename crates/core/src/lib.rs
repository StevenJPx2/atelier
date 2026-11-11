//! atelier's core: the issue machine and the roles plugins fill. It does no
//! I/O, has no dependencies, and knows no language, tracker, forge or agent.

mod brief;
mod machine;
mod plugin;
mod poll;
mod roles;
mod seen;
mod settings;

pub use brief::Brief;
pub use machine::{Action, Event, Mark, Phase, Pipeline, PipelineError, Reason, step};
pub use plugin::{MakeTracker, MissingPlugin, Plugin, Registry, Role};
pub use poll::{
    Observation, Observed, Plugins, Project, Record, Report, Source, State, Step, act, observe,
};
pub use roles::{
    Agent, Failure, Filter, Forge, Issue, IssueKey, Listing, PluginError, Repo, Session, Tracker,
    WorktreeRequest, Worktrees,
};
pub use seen::{Checks, Marks, PullRequest, Seen, StageOutcome, events};
pub use settings::{Setting, Settings, SettingsError};
