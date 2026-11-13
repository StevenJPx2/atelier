//! `git` as a plugin: the `git.worktree` kind of step.

use atelier_core::{Kind, Outcomes, Plugin, Registry, Settings, SettingsError, Step, Tree};

use crate::Git;

impl Plugin for Git {
    fn name(&self) -> &'static str {
        "git"
    }

    fn register<'a>(&'a self, registry: &mut Registry<'a>) {
        registry.step(self);
    }
}

impl Step for Git {
    /// `worktree`: the run's worktree, made from `origin/<base>`.
    fn kinds(&self) -> Vec<Kind> {
        vec![Kind {
            name: "worktree",
            outcomes: Outcomes::Own(&["passed"]),
            resident: false,
        }]
    }

    /// A worktree takes no settings of its own.
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
