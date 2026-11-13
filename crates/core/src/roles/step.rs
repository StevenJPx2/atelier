use std::path::{Path, PathBuf};

use super::{Issue, PluginError, Repo, Session};
use crate::{Brief, PullRequest, Settings, SettingsError, StepSpec, Tree};

/// A kind of step a plugin offers pipelines, such as `checks` for
/// `github.checks`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kind {
    pub name: &'static str,
    pub outcomes: Outcomes,
    /// It stays alive once it settles, as an agent's session does, so later
    /// steps can talk to it; every way out of a run stops it.
    pub resident: bool,
}

/// The outcomes a step of a kind settles with, besides `failed`, which
/// every step has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcomes {
    /// The kind's own; a step cannot change them.
    Own(&'static [&'static str]),
    /// Those the step names in `outcomes`, or `passed` when it names none,
    /// as an agent's step chooses its own.
    Named,
}

/// What a step settled with, and why, as a run's closing comment lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub name: String,
    pub why: String,
}

/// What a start left for the run to keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
    Nothing,
    /// The run's worktree.
    Worktree(PathBuf),
    /// A resident step's session, which later steps talk to.
    Session(Session),
    /// Words for each resident step's session, as `forge.ready` tells the
    /// builder every check passed.
    Tell(String),
}

/// What a poll sees of a started step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// It has not settled.
    Running,
    /// It settled with one of its outcomes.
    Settled(Outcome),
    /// A failed round; `last` when no round can go better, as when its pull
    /// request closed.
    Failed { why: String, last: bool },
    /// A settled step runs again, as checks do after a push.
    Restarted,
}

/// What a step knows of its run when it starts or is observed.
#[derive(Debug, Clone, Copy)]
pub struct RunContext<'a> {
    pub repo: &'a Repo,
    pub local: &'a Path,
    pub base: &'a str,
    pub issue: &'a Issue,
    /// The worktree's folder name, such as `issue-12`.
    pub name: &'a str,
    pub branch: &'a str,
    pub worktree: Option<&'a Path>,
    /// The step's own folder, for what it keeps between polls.
    pub folder: &'a Path,
    /// The builder's brief for this item, for a step that starts an agent.
    pub brief: &'a Brief,
    /// The step's session from its last start, if it made one.
    pub session: Option<&'a Session>,
    /// What the forge last reported of the pull request on the branch.
    pub pull_request: Option<PullRequest>,
    /// The rounds it failed so far.
    pub failed: u8,
    /// It has settled, and is asked only whether it runs again.
    pub settled: bool,
}

/// A plugin's kinds of step.
pub trait Step {
    /// The kinds it offers.
    fn kinds(&self) -> Vec<Kind>;

    /// Checks one step's own settings, every key but those all steps share,
    /// when the config loads, and returns them as the step runs with them.
    /// `tree` is the project at `origin/<base>`, where a setting such as
    /// `prompt` names a file.
    ///
    /// # Errors
    ///
    /// A setting the kind cannot use, named by its key.
    fn configure(
        &self,
        kind: &str,
        settings: &Settings,
        tree: &dyn Tree,
    ) -> Result<Settings, SettingsError>;

    /// Starts the step for a run, or starts it again after a failed round,
    /// and returns at once; a later poll observes it.
    ///
    /// # Errors
    ///
    /// The plugin's failure; a permanent one blocks the run.
    fn start(&self, run: &RunContext<'_>, step: &StepSpec) -> Result<Started, PluginError>;

    /// What a poll sees of the step: whether it settled, failed a round,
    /// or, once settled, runs again.
    ///
    /// # Errors
    ///
    /// The plugin's failure.
    fn observe(&self, run: &RunContext<'_>, step: &StepSpec) -> Result<Observed, PluginError>;
}
