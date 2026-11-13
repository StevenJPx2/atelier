//! atelier's core: the pipelines a project writes, the machine that runs
//! them, the roles plugins fill, and which gates a change needs. Its one
//! dependency is rungs, the step machine; it knows no language, tracker,
//! forge or agent.

mod brief;
mod checks;
mod forge_steps;
mod machine;
mod pipeline;
mod plugin;
mod poll;
mod roles;
mod seen;
mod settings;

pub use brief::Brief;
pub use checks::{
    Checks, Fix, Gate, MemoryTree, Override, Overrides, Planned, Root, Tree, fixes, gates,
};
pub use forge_steps::{forge_kinds, observe_builder, observe_forge_step, start_forge_step};
pub use machine::{Action, Event, Mark, Phase, Reason, Run, step};
pub use pipeline::{
    FAILED, Kinds, MAX_STEPS, Need, Needs, PASSED, Pipeline, PipelineError, StepSpec,
};
pub use plugin::{MakeChecks, MakeTracker, MissingPlugin, On, Plugin, Registry, Role};
pub use poll::{
    Move, Observation, Plugins, Project, Record, Report, Source, State, StepRecord, Watched, act,
    observe,
};
pub use roles::{
    Agent, CiFile, Failure, Filter, Forge, Issue, IssueKey, Kind, Listing, Observed, Outcome,
    Outcomes, PluginError, Repo, RunContext, Session, Started, Step, Tracker, WorktreeRequest,
    Worktrees,
};
pub use seen::{Marks, PullRequest, Rollup, Seen};
pub use settings::{Setting, Settings, SettingsError};

/// The step machine pipelines run on.
pub use rungs;
