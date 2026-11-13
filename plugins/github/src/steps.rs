//! GitHub's kinds of step, as the forge: `checks`, `ready` and `merge`,
//! read from the pull request on the run's branch.

use atelier_core::{
    Kind, Observed, PluginError, RunContext, Settings, SettingsError, Started, Step, StepSpec,
    Tree, forge_kinds, observe_forge_step, start_forge_step,
};

use crate::GitHub;

impl Step for GitHub {
    /// Every forge's kinds: `checks`, `ready` and `merge`.
    fn kinds(&self) -> Vec<Kind> {
        forge_kinds()
    }

    /// None of them takes settings of its own.
    fn configure(
        &self,
        _: &str,
        settings: &Settings,
        _: &dyn Tree,
    ) -> Result<Settings, SettingsError> {
        settings.only("", &[])?;

        Ok(settings.clone())
    }

    fn start(&self, run: &RunContext<'_>, step: &StepSpec) -> Result<Started, PluginError> {
        start_forge_step(&step.kind, run)
    }

    fn observe(&self, run: &RunContext<'_>, step: &StepSpec) -> Result<Observed, PluginError> {
        observe_forge_step(&step.kind, run)
    }
}
