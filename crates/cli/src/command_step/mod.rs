//! `atelier.command`: a shell command a pipeline runs in the run's
//! worktree, with a time limit.

pub mod run;

use atelier_core::{
    FAILED, Kind, Observed, Outcomes, Plugin, PluginError, Registry, RunContext, Settings,
    SettingsError, Started, Step, StepSpec, Tree,
};

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

    /// Starts the command in the run's worktree, in the background.
    fn start(&self, run: &RunContext<'_>, step: &StepSpec) -> Result<Started, PluginError> {
        let worktree = run.worktree.ok_or_else(|| {
            PluginError::permanent("a command runs in the run's worktree; git.worktree makes it")
        })?;

        run::start(run.folder, worktree, run.base, &step.settings)?;
        Ok(Started::Nothing)
    }

    /// How the command ended, once it has.
    fn observe(&self, run: &RunContext<'_>, step: &StepSpec) -> Result<Observed, PluginError> {
        Ok(match run::observe(run.folder, &step.settings)? {
            None => Observed::Running,
            Some(outcome) if outcome.name == FAILED => Observed::Failed {
                why: outcome.why,
                last: false,
            },
            Some(outcome) => Observed::Settled(outcome),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use atelier_core::{Brief, Issue, IssueKey, Marks, MemoryTree, Needs, Repo, Setting};

    use super::*;

    fn command(run: &str) -> StepSpec {
        StepSpec {
            id: "deps".into(),
            uses: "atelier.command".into(),
            plugin: "atelier",
            kind: "command".into(),
            needs: Needs::default(),
            outcomes: vec!["passed".into()],
            rounds: 1,
            restart: false,
            resident: false,
            settings: Settings::default().with("run", Setting::Text(run.into())),
        }
    }

    /// Calls `with` on a run in `worktree`, keeping the step's files in
    /// `folder`.
    fn with_run<T>(
        worktree: Option<&Path>,
        folder: &Path,
        with: impl FnOnce(&RunContext<'_>) -> T,
    ) -> T {
        let issue = Issue {
            key: IssueKey {
                tracker: "github".into(),
                id: "40".into(),
            },
            title: "Audit".into(),
            open: true,
            marks: Marks::default(),
        };
        let brief = Brief::new("#40", &issue, "issue-40", "main");
        let repo = Repo("StevenJPx2/atelier".into());
        let run = RunContext {
            repo: &repo,
            local: Path::new("/clones/atelier.git"),
            base: "main",
            issue: &issue,
            name: "issue-40",
            branch: "issue-40",
            worktree,
            folder,
            brief: &brief,
            session: None,
            pull_request: None,
            failed: 0,
            settled: false,
        };

        with(&run)
    }

    /// What a poll sees once the command ended, within ten seconds.
    fn ended(worktree: &Path, folder: &Path, step: &StepSpec) -> Observed {
        let begun = Instant::now();

        loop {
            let seen = with_run(Some(worktree), folder, |run| Atelier.observe(run, step)).unwrap();

            if seen != Observed::Running || begun.elapsed() > Duration::from_secs(10) {
                return seen;
            }

            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn a_command_step_runs_in_the_worktree_and_settles_how_it_exits() {
        let root = std::env::temp_dir().join(format!("atelier-step-{}", std::process::id()));
        let worktree = root.join("worktree");
        std::fs::create_dir_all(&worktree).unwrap();
        let (pass, fail) = (command("true"), command("echo advisory; exit 2"));

        let started = with_run(Some(&worktree), &root.join("pass"), |run| {
            Atelier.start(run, &pass)
        });
        let passed = ended(&worktree, &root.join("pass"), &pass);
        with_run(Some(&worktree), &root.join("fail"), |run| {
            Atelier.start(run, &fail)
        })
        .unwrap();
        let failed = ended(&worktree, &root.join("fail"), &fail);
        let homeless = with_run(None, &root, |run| Atelier.start(run, &pass));
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(started, Ok(Started::Nothing));
        assert_eq!(
            passed,
            Observed::Settled(atelier_core::Outcome {
                name: "passed".into(),
                why: "it exited 0".into(),
            })
        );
        assert_eq!(
            failed,
            Observed::Failed {
                why: "it exited 2:\nadvisory".into(),
                last: false,
            }
        );
        assert!(homeless.unwrap_err().is_permanent());
    }

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
