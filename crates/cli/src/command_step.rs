//! `atelier.command`: a shell command a pipeline runs in the run's
//! worktree, with a time limit.

use atelier_core::{Kind, Outcomes, Plugin, Registry, Settings, SettingsError, Step, Tree};

/// A command's time limit when its step sets none.
pub const DEFAULT_TIMEOUT_SECS: i64 = 600;

/// The longest time limit a step may set: a day.
const MAX_TIMEOUT_SECS: i64 = 86_400;

/// atelier's own kinds of step, under the namespace `atelier`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Atelier;

impl Plugin for Atelier {
    fn name(&self) -> &'static str {
        "atelier"
    }

    fn register<'a>(&'a self, registry: &mut Registry<'a>) {
        registry.step(self);
    }
}

impl Step for Atelier {
    /// `command`: passes when its command exits 0.
    fn kinds(&self) -> Vec<Kind> {
        vec![Kind {
            name: "command",
            outcomes: Outcomes::Own(&["passed"]),
            resident: false,
        }]
    }

    /// A command takes `run`, the shell command, and `timeout`, its limit
    /// in seconds, 600 unless set, at most a day.
    fn configure(
        &self,
        _: &str,
        settings: &Settings,
        _: &dyn Tree,
    ) -> Result<Settings, SettingsError> {
        settings.only("", &["run", "timeout"])?;

        settings
            .text("", "run")?
            .filter(|run| !run.trim().is_empty())
            .ok_or_else(|| SettingsError::new("", "run", "must be a command"))?;

        let timeout = settings
            .number("", "timeout")?
            .unwrap_or(DEFAULT_TIMEOUT_SECS);

        if !(1..=MAX_TIMEOUT_SECS).contains(&timeout) {
            return Err(SettingsError::new(
                "",
                "timeout",
                &format!("is {timeout}; it must be from 1 to {MAX_TIMEOUT_SECS} seconds"),
            ));
        }

        Ok(settings.clone())
    }
}

#[cfg(test)]
mod tests {
    use atelier_core::{MemoryTree, Setting};

    use super::*;

    fn configure(settings: &Settings) -> Result<Settings, String> {
        Atelier
            .configure("command", settings, &MemoryTree::default())
            .map_err(|error| error.to_string())
    }

    #[test]
    fn a_command_takes_a_command_and_a_time_limit() {
        let run = Settings::default().with("run", Setting::Text("cargo deny check".into()));
        let timeout = |secs| run.clone().with("timeout", Setting::Number(secs));

        assert_eq!(Atelier.name(), "atelier");
        assert_eq!(Atelier.kinds()[0].name, "command");
        assert_eq!(configure(&run), Ok(run.clone()));
        assert_eq!(configure(&timeout(1)), Ok(timeout(1)));
        assert_eq!(configure(&timeout(86_400)), Ok(timeout(86_400)));
        assert_eq!(
            configure(&timeout(0)),
            Err("timeout: is 0; it must be from 1 to 86400 seconds".into())
        );
        assert_eq!(
            configure(&timeout(86_401)),
            Err("timeout: is 86401; it must be from 1 to 86400 seconds".into())
        );
        assert_eq!(
            configure(&Settings::default().with("run", Setting::Text("  ".into()))),
            Err("run: must be a command".into())
        );
        assert_eq!(
            configure(&run.with("cwd", Setting::Text("docs".into()))),
            Err("cwd: unknown; it takes run, timeout".into())
        );
    }
}
