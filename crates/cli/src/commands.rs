//! `atelier init`, `add` and `status`.

use std::path::Path;

use atelier_core::rungs::Status;
use atelier_core::{CiFile, Phase, Pipeline, Reason, Record, Repo};
use atelier_git::{Repository, WorkingTree};
use atelier_github::GitHub;
use color_eyre::eyre::{Result, WrapErr, bail, eyre};

use crate::clone;
use crate::config::{self, repo_from_url};
use crate::paths::Paths;
use crate::plugins::Build;
use crate::projects::{self, Entry};
use crate::state;

/// The checkout at `path`, and the project its `origin` names.
pub fn here(path: &Path) -> Result<(Repository, Entry)> {
    let path = path
        .canonicalize()
        .wrap_err_with(|| format!("{} is not a folder", path.display()))?;
    let local = Repository::new(&path);
    let url = local.origin()?;
    let repo = repo_from_url(&url).ok_or_else(|| eyre!("cannot read a repository from {url}"))?;

    Ok((
        local,
        Entry {
            repo,
            path: None,
            filter: std::collections::BTreeMap::default(),
        },
    ))
}

/// The commit a release build was made from; none in any other build.
const RELEASE_COMMIT: Option<&str> = option_env!("ATELIER_COMMIT");

/// Makes each tracker's labels, and writes the forge's CI for the gates
/// when this atelier is a release.
///
/// # Errors
///
/// The project's settings are not valid, its tracker refuses, or a CI file
/// cannot be written.
pub fn init(paths: &Paths, path: &Path) -> Result<()> {
    let (local, entry) = here(path)?;
    let resolved = config::resolve(&entry, &local)?;
    let repo = Repo(entry.repo.clone());
    let build = Build::new(paths);
    let forge = build.forge(&resolved.forge)?;

    match RELEASE_COMMIT {
        Some(commit) => {
            let release = env!("CARGO_PKG_VERSION");
            let files = forge.ci(release, commit, &resolved.project.base);
            let top = WorkingTree::read(path)?;

            for written in write_ci(top.root(), &files)? {
                println!("{}: wrote {written}", entry.repo);
            }
        }
        None => eprintln!(
            "{}: this atelier is no release build, so it writes no CI",
            entry.repo
        ),
    }

    for spec in &resolved.trackers {
        if spec.plugin != "github" {
            bail!(
                "this atelier has no tracker plugin \"{}\"; it has github",
                spec.plugin
            );
        }

        let tracker = GitHub::new()
            .tracker(&spec.settings)
            .map_err(|error| eyre!("the github tracker: {error}"))?;
        tracker.create_labels(&repo)?;

        let names: Vec<&str> = tracker
            .labels()
            .all()
            .iter()
            .map(|label| label.name.as_str())
            .collect();
        println!("{}: labels {}", entry.repo, names.join(", "));
    }

    Ok(())
}

/// Writes each CI file under `top`, returning their paths.
fn write_ci(top: &Path, files: &[CiFile]) -> Result<Vec<String>> {
    files
        .iter()
        .map(|file| {
            let path = top.join(&file.path);

            if let Some(folder) = path.parent() {
                std::fs::create_dir_all(folder)?;
            }

            std::fs::write(&path, &file.text)
                .wrap_err_with(|| format!("writing {}", path.display()))?;
            Ok(file.path.clone())
        })
        .collect()
}

/// Watches `repo`, making atelier's own clone of it. A clone made for a
/// repository it cannot reach is removed again.
///
/// # Errors
///
/// `repo` is not `owner/name`, cannot be reached, or is already watched.
pub fn add(paths: &Paths, repo: &str) -> Result<()> {
    let fresh = !clone::of(paths, repo)?.exists();
    let local = clone::ensure(paths, repo)?;

    if let Err(error) = local.default_branch() {
        if fresh {
            let _ = std::fs::remove_dir_all(local.path());
        }
        return Err(error).wrap_err_with(|| format!("cannot reach {repo}"));
    }

    projects::add(&paths.projects(), repo)?;
    println!("watching {repo}, cloned in {}", local.path().display());

    Ok(())
}

/// Prints each project's settings, where each came from, and each issue
/// with its pull request, from the state alone: it asks no forge.
///
/// # Errors
///
/// `projects.toml` or `state.json` cannot be read.
pub fn status(paths: &Paths) -> Result<()> {
    let states = state::load(&paths.state_file())?;

    for entry in projects::read(&paths.projects())? {
        let local = clone::of(paths, &entry.repo)?;
        let mut pipelines = Vec::new();
        println!("{}  {}", entry.repo, local.path().display());

        if local.exists() {
            let build = Build::new(paths);
            let resolved = config::resolve(&entry, &local).and_then(|resolved| {
                build.checks(&resolved)?;
                let tree = local.tree(&resolved.project.base)?;
                let pipelines = build.pipelines(&resolved, &tree)?;

                Ok((resolved, pipelines))
            });

            match resolved {
                Ok((resolved, read)) => {
                    for (name, value, origin) in &resolved.shown {
                        println!("  {name:<11} {value:<24} {origin}");
                    }

                    show(&read, &resolved.pipelines);
                    pipelines = read;
                }
                Err(error) => println!("  settings: {error:#}"),
            }
        } else {
            println!("  settings: not cloned yet; the daemon clones it");
        }

        let Some(state) = states.get(&entry.repo) else {
            continue;
        };

        for (key, record) in &state.runs {
            let pipeline = pipelines
                .iter()
                .find(|pipeline| pipeline.name() == record.pipeline);

            for line in run(&key.to_string(), record, pipeline) {
                println!("{line}");
            }
        }
    }

    Ok(())
}

/// One run's lines: its item, pipeline and phase, its branch, session and
/// pull request, then each step's status.
fn run(key: &str, record: &Record, pipeline: Option<&Pipeline>) -> Vec<String> {
    let phase = format!("{} {}", record.pipeline, phase(record));
    let session = record
        .sessions()
        .next()
        .map_or("-", |session| session.id.as_str());
    let pull_request = record
        .pull_request
        .map_or_else(|| "-".to_owned(), |number| format!("#{number}"));

    let mut lines = vec![format!(
        "  {key:<11} {phase:<24} {}  {session}  {pull_request}",
        record.branch
    )];

    let steps: Vec<String> = record
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let spec = pipeline.and_then(|pipeline| pipeline.steps().get(index));
            let failed = record.progress.failed.get(index).copied().unwrap_or(0);
            let status = match record.progress.status.get(index) {
                Some(Status::Settled(outcome)) => spec
                    .and_then(|spec| spec.outcome_name(*outcome))
                    .map_or_else(|| format!("outcome {}", outcome.0), str::to_owned),
                Some(Status::Running) => "running".to_owned(),
                Some(Status::Skipped) => "skipped".to_owned(),
                Some(Status::Pending) | None => "pending".to_owned(),
            };
            let rounds = match failed {
                0 => String::new(),
                1 => " (1 failed round)".to_owned(),
                _ => format!(" ({failed} failed rounds)"),
            };

            format!("{} {status}{rounds}", step.id)
        })
        .collect();

    if !steps.is_empty() {
        lines.push(format!("    {}", steps.join(", ")));
    }

    lines
}

/// The run's phase as a person reads it, such as `Blocked at checks`.
fn phase(record: &Record) -> String {
    match record.phase {
        Phase::Blocked(Reason::Failed(step)) => format!("Blocked at {}", record.step_id(step)),
        Phase::Blocked(Reason::Start(step)) => {
            format!("Blocked starting {}", record.step_id(step))
        }
        Phase::Blocked(Reason::Claim) => "Blocked claiming".to_owned(),
        Phase::Blocked(Reason::Changed) => "Blocked, changed".to_owned(),
        Phase::Blocked(Reason::LostTrack) => "Blocked, lost track".to_owned(),
        other => format!("{other:?}"),
    }
}
/// Each pipeline, where it came from, and each step's kind, with the
/// plugin a role's kind resolved to, such as `forge.checks → github.checks`.
fn show(pipelines: &[Pipeline], written: &[config::Written]) {
    for (pipeline, written) in pipelines.iter().zip(written) {
        println!(
            "  {:<11} {:<24} {}",
            "pipeline",
            pipeline.name(),
            written.origin
        );

        for step in pipeline.steps() {
            let resolved = step.resolved();
            let kind = if resolved == step.uses {
                resolved
            } else {
                format!("{} → {resolved}", step.uses)
            };

            println!("    {:<9} {kind}", step.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_shows_its_phase_and_each_step_s_status_and_failed_rounds() {
        use atelier_core::rungs::{Outcome, Progress};
        use atelier_core::{IssueKey, StepRecord};

        let key = IssueKey {
            tracker: "github".into(),
            id: "12".into(),
        };
        let mut record = Record::new(&key, "Fix the dead link", "build");
        let mut progress = Progress::default();
        progress.status[0] = Status::Settled(Outcome(0));
        progress.status[1] = Status::Running;
        progress.status[2] = Status::Skipped;
        progress.status[3] = Status::Settled(Outcome::FAILED);
        progress.failed[1] = 1;
        progress.failed[3] = 2;
        record.phase = Phase::Running;
        record.progress = progress;
        record.pull_request = Some(8);
        record.steps = ["worktree", "checks", "lint", "merge", "late"]
            .map(|id| StepRecord {
                id: id.into(),
                plugin: String::new(),
                session: None,
                why: None,
            })
            .to_vec();

        let lines = run("github#12", &record, None);
        let phases: Vec<String> = [
            Phase::Blocked(Reason::Failed(3)),
            Phase::Blocked(Reason::Start(1)),
            Phase::Blocked(Reason::Claim),
            Phase::Blocked(Reason::Changed),
            Phase::Blocked(Reason::LostTrack),
            Phase::Done,
        ]
        .into_iter()
        .map(|phase| {
            self::phase(&Record {
                phase,
                ..record.clone()
            })
        })
        .collect();

        assert_eq!(
            lines,
            [
                "  github#12   build Running            issue-12-fix-the-dead-link  -  #8",
                "    worktree outcome 0, checks running (1 failed round), lint skipped, \
                 merge outcome 127 (2 failed rounds), late pending",
            ]
        );
        assert_eq!(
            phases,
            [
                "Blocked at merge",
                "Blocked starting checks",
                "Blocked claiming",
                "Blocked, changed",
                "Blocked, lost track",
                "Done",
            ]
        );
    }

    #[test]
    fn each_ci_file_is_written_in_its_folder_under_the_top() {
        let top = std::env::temp_dir().join(format!("atelier-ci-{}", std::process::id()));
        let files = [CiFile {
            path: ".github/workflows/atelier.yml".into(),
            text: "name: atelier\n".into(),
        }];

        let written = write_ci(&top, &files).unwrap();
        let text = std::fs::read_to_string(top.join(".github/workflows/atelier.yml"));
        let _ = std::fs::remove_dir_all(&top);

        assert_eq!(written, [".github/workflows/atelier.yml"]);
        assert_eq!(text.unwrap(), "name: atelier\n");
    }
}
