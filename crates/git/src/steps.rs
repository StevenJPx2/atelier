//! `git` as a plugin: the `git.worktree` kind of step.

use atelier_core::{
    Kind, Observed, Outcome, Outcomes, Plugin, PluginError, Registry, RunContext, Settings,
    SettingsError, Started, Step, StepSpec, Tree, WorktreeRequest, Worktrees,
};

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

    /// Makes the run's worktree on its branch, or reuses it.
    fn start(&self, run: &RunContext<'_>, _: &StepSpec) -> Result<Started, PluginError> {
        let request = WorktreeRequest {
            repo: run.repo,
            local: run.local,
            name: run.name,
            branch: run.branch,
            base: run.base,
        };

        Ok(Started::Worktree(self.create(&request)?))
    }

    /// Passed once the run keeps its worktree.
    fn observe(&self, run: &RunContext<'_>, _: &StepSpec) -> Result<Observed, PluginError> {
        Ok(run.worktree.map_or(Observed::Running, |worktree| {
            Observed::Settled(Outcome {
                name: "passed".to_owned(),
                why: format!("made {}", worktree.display()),
            })
        }))
    }
}
