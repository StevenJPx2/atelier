//! GitHub's kinds of step, as the forge: `checks`, `ready` and `merge`.

use atelier_core::{Kind, Outcomes, Settings, SettingsError, Step, Tree};

use crate::GitHub;

const PASSED: Outcomes = Outcomes::Own(&["passed"]);

impl Step for GitHub {
    /// `checks` passes when every check on the pull request's head passed,
    /// `ready` when the pull request is marked ready, and `merge` settles
    /// `merged` once a person merges it.
    fn kinds(&self) -> Vec<Kind> {
        let kind = |name, outcomes| Kind {
            name,
            outcomes,
            resident: false,
        };

        vec![
            kind("checks", PASSED),
            kind("ready", PASSED),
            kind("merge", Outcomes::Own(&["merged"])),
        ]
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
}
