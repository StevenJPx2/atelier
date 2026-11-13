//! `OpenCode`'s kind of step, as the agent: `session`.

use atelier_core::{
    Agent, Kind, Observed, Outcomes, PluginError, RunContext, Setting, Settings, SettingsError,
    Started, Step, StepSpec, Tree, observe_builder,
};

use crate::OpenCode;

/// The one brief core writes itself: the builder's contract.
const BUILDER: &str = "builder";

impl Step for OpenCode {
    /// `session`: a session that stays once it settles, settling with the
    /// outcomes its step names.
    fn kinds(&self) -> Vec<Kind> {
        vec![Kind {
            name: "session",
            outcomes: Outcomes::Named,
            resident: true,
        }]
    }

    /// A session takes `prompt`, a file on the base branch, or
    /// `brief = "builder"`. The prompt's text is kept under `text`, as the
    /// config read it.
    fn configure(
        &self,
        _: &str,
        settings: &Settings,
        tree: &dyn Tree,
    ) -> Result<Settings, SettingsError> {
        settings.only("", &["prompt", "brief"])?;

        match (settings.text("", "prompt")?, settings.text("", "brief")?) {
            (Some(prompt), None) => {
                let text = tree.read(prompt).ok_or_else(|| {
                    SettingsError::new("", "prompt", &format!("{prompt} is not on the base branch"))
                })?;

                Ok(settings.clone().with("text", Setting::Text(text)))
            }
            (None, Some(BUILDER)) => Ok(settings.clone()),
            (None, Some(_)) => Err(SettingsError::new("", "brief", "must be \"builder\"")),
            (Some(_), Some(_)) => Err(SettingsError::new(
                "",
                "brief",
                "goes without prompt; a session takes one of them",
            )),
            (None, None) => Err(SettingsError::new(
                "",
                "prompt",
                "is missing; a session takes a prompt file, or brief = \"builder\"",
            )),
        }
    }

    /// Starts a session in the run's worktree with its prompt, or the
    /// builder's brief; starting again replaces the step's last session.
    fn start(&self, run: &RunContext<'_>, step: &StepSpec) -> Result<Started, PluginError> {
        let worktree = run.worktree.ok_or_else(|| {
            PluginError::permanent("a session works in the run's worktree; git.worktree makes it")
        })?;
        let brief = match step.settings.text("", "text") {
            Ok(Some(prompt)) => {
                let mut brief = run.brief.prompted(prompt);
                brief.title = format!("{} [{}]", brief.title, step.id);
                brief
            }
            _ => run.brief.clone(),
        };

        Ok(Started::Session(Agent::start(
            self,
            worktree,
            &brief,
            run.session,
        )?))
    }

    /// The builder settles `opened` once its pull request is open.
    fn observe(&self, run: &RunContext<'_>, step: &StepSpec) -> Result<Observed, PluginError> {
        let builder = step.settings.text("", "brief").ok().flatten() == Some(BUILDER);

        Ok(if builder {
            observe_builder(run)
        } else {
            Observed::Running
        })
    }
}
