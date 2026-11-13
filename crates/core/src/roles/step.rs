use crate::{Settings, SettingsError, Tree};

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
}
