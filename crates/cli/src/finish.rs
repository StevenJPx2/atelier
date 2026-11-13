//! `atelier finish`: find the run by its worktree, choose its running
//! agent step, and leave a request for the worker.

use std::path::Path;

use atelier_core::rungs::Status;
use atelier_core::{Command, Outcome, Phase, Pipeline, Record, Request, StepSpec};
use color_eyre::eyre::{Result, bail, eyre};

use crate::paths::Paths;
use crate::plugins::Build;
use crate::{clone, config, projects, requests};

/// Leaves a request to settle a running agent step. A unique running
/// agent step with the outcome is inferred; an ambiguous one needs
/// `--step`. Only the daemon writes the state.
///
/// # Errors
///
/// There is no working run here, its pipeline changed, the step or
/// outcome is not declared, or the queue cannot take the request.
pub fn run(paths: &Paths, path: &Path, name: &str, why: &str, step: Option<&str>) -> Result<()> {
    if why.trim().is_empty() || why.len() > 4096 {
        bail!("why must be nonempty and at most 4096 bytes");
    }

    let target = requests::target::in_worktree(paths, path)?;
    let entries = projects::read(&paths.projects())?;
    let entry = entries
        .iter()
        .find(|entry| entry.repo == target.repo)
        .ok_or_else(|| eyre!("{} is not watched", target.repo))?;
    let local = clone::of(paths, &target.repo)?;
    let resolved = config::resolve(entry, &local)?;
    let build = Build::new(paths);
    let pipelines = build.pipelines(&resolved, &local.tree(&resolved.project.base)?)?;
    let pipeline = pipelines
        .iter()
        .find(|pipeline| pipeline.name() == target.record.pipeline)
        .ok_or_else(|| eyre!("this run's pipeline is not configured"))?;
    let (index, spec) = choose(&target.record, pipeline, name, step)?;
    let kept = target
        .record
        .steps
        .get(index)
        .ok_or_else(|| eyre!("the run has no such step"))?;
    let session = kept.session.as_ref().ok_or_else(|| {
        eyre!(
            "{} is still starting; try finish once its session starts",
            spec.id
        )
    })?;
    let failed = target
        .record
        .progress
        .failed
        .get(index)
        .copied()
        .ok_or_else(|| eyre!("the run has no such progress"))?;
    let request = Request {
        generation: target.record.generation,
        command: Command::Finish {
            step: spec.id.clone(),
            outcome: Outcome {
                name: name.to_owned(),
                why: why.to_owned(),
            },
            session: session.id.clone(),
            failed,
        },
    };

    requests::submit(
        &requests::folder(paths, &target.repo),
        &target.key,
        &request,
    )?;
    println!(
        "requested {} {}: {} settled {name}",
        target.record.pipeline, target.key, spec.id
    );
    Ok(())
}

/// A running resident step that declares `name`, with an explicit id
/// when several match.
fn choose<'a>(
    record: &Record,
    pipeline: &'a Pipeline,
    name: &str,
    id: Option<&str>,
) -> Result<(usize, &'a StepSpec)> {
    if record.phase != Phase::Running
        || !record.follows(pipeline)
        || pipeline.plan().resume(record.progress).is_err()
    {
        bail!("this run is not running on that pipeline");
    }

    let choices: Vec<_> = pipeline
        .steps()
        .iter()
        .enumerate()
        .filter(|(index, spec)| {
            record.progress.status.get(*index) == Some(&Status::Running)
                && spec.resident
                && spec.outcome(name).is_some()
                && id.is_none_or(|id| spec.id == id)
        })
        .collect();

    match choices.as_slice() {
        [only] => Ok(*only),
        [] => bail!(
            "no running agent step{} declares outcome {name}",
            id.map_or_else(String::new, |id| format!(" named {id}"))
        ),
        several => {
            let names: Vec<&str> = several.iter().map(|(_, spec)| spec.id.as_str()).collect();
            bail!(
                "outcome {name} matches {}; use --step <id>",
                names.join(", ")
            )
        }
    }
}
#[cfg(test)]
mod tests {
    use atelier_core::rungs::Outcome;
    use atelier_core::{IssueKey, Kinds, MemoryTree, PipelineError, Setting, Settings};

    use super::*;

    fn pipeline() -> Result<Pipeline, PipelineError> {
        let git = atelier_git::Git::new("/unused");
        let agent = atelier_opencode::OpenCode::new();
        let kinds = Kinds::default()
            .with("git", "git", &git)
            .with("agent", "opencode", &agent);
        let worktree = Settings::default()
            .with("id", Setting::Text("worktree".into()))
            .with("uses", Setting::Text("git.worktree".into()));
        let builder = Settings::default()
            .with("id", Setting::Text("builder".into()))
            .with("uses", Setting::Text("agent.session".into()))
            .with("brief", Setting::Text("builder".into()))
            .with(
                "outcomes",
                Setting::List(vec![Setting::Text("nothing".into())]),
            );

        Pipeline::read(
            "build",
            &[worktree, builder],
            &kinds,
            &MemoryTree::default(),
        )
    }

    #[test]
    fn finish_requires_a_running_run_matching_the_plan_and_reachable_progress() {
        let pipeline = pipeline().unwrap();
        let mut run = pipeline.plan().start();
        run.advance();
        run.settle(0, Outcome(0)).unwrap();
        run.advance();
        let key = IssueKey {
            tracker: "github".into(),
            id: "12".into(),
        };
        let mut record = Record::new(&key, "Fix", "build");
        record.begin(&pipeline);
        record.phase = Phase::Running;
        record.progress = run.progress();

        assert_eq!(
            choose(&record, &pipeline, "nothing", None).unwrap().1.id,
            "builder"
        );
        assert!(choose(&record, &pipeline, "nothing", Some("absent")).is_err());

        let mut claiming = record.clone();
        claiming.phase = Phase::Claiming;
        let mut changed = record.clone();
        changed.steps[0].id = "changed".into();
        let mut unreachable = record;
        unreachable.progress.status[0] = Status::Pending;

        for invalid in [claiming, changed, unreachable] {
            assert!(choose(&invalid, &pipeline, "nothing", None).is_err());
        }
    }
}
