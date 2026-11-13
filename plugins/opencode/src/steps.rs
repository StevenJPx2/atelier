//! `OpenCode`'s kind of step, as the agent: `session`.

use atelier_core::{Kind, Outcomes, Setting, Settings, SettingsError, Step, Tree};

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
}
