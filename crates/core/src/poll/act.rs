use std::path::PathBuf;

use rungs::{Outcome, Status};

use super::{Observation, Plugins, Project, Record, State, Watched};
use crate::{
    Action, Brief, Event, IssueKey, Observed, Phase, Pipeline, PluginError, PullRequest, Reason,
    Run, RunContext, Seen, Started, Tracker, step,
};

/// The pipeline the build mark starts.
pub const TRIGGERED: &str = "build";

/// The most moves one run makes in one poll. A start takes a few for each
/// step, so this only stops a fault from looping.
const MAX_MOVES: usize = 64;

/// What one poll did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub moves: Vec<Move>,
    pub failures: Vec<(IssueKey, PluginError)>,
}

/// One run's move on one event, such as `builder settled opened`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    pub key: IssueKey,
    pub pipeline: String,
    pub from: Phase,
    pub to: Phase,
    pub what: String,
}

struct Context<'p, 'a> {
    project: &'p Project,
    plugins: &'p Plugins<'a>,
}

/// An event, and why, when a step's observation said.
type Found = (Event, Option<String>);

/// Moves every observed item's run and carries out its actions through
/// the plugins.
///
/// `save` runs after each move and each action, so a stop between
/// actions loses nothing; the next poll finishes what is pending. One
/// item's failure is reported and leaves the others to go on. The
/// trackers' listing tags are kept only when every item succeeded, so an
/// item that failed is listed again.
///
/// # Errors
///
/// The first failure to save the state.
pub fn act<E>(
    observation: &Observation,
    state: &mut State,
    project: &Project,
    plugins: &Plugins<'_>,
    save: &mut impl FnMut(&State) -> Result<(), E>,
) -> Result<Report, E> {
    let context = Context { project, plugins };
    let mut report = Report::default();

    for (key, watched) in &observation.issues {
        if let Err(error) = drive(
            watched,
            observation.requests.get(key).map_or(&[], Vec::as_slice),
            state,
            &context,
            save,
            &mut report.moves,
        )? {
            report.failures.push((key.clone(), error));
        }
    }

    if report.failures.is_empty() {
        state.etags.extend(observation.etags.clone());
        save(state)?;
    }

    Ok(report)
}

fn drive<E>(
    watched: &Watched,
    requests: &[super::Request],
    state: &mut State,
    context: &Context<'_, '_>,
    save: &mut impl FnMut(&State) -> Result<(), E>,
    moves: &mut Vec<Move>,
) -> Result<Result<(), PluginError>, E> {
    let issue = &watched.issue;
    let mut seen = watched.seen;

    remember_pull_request(watched, state, save)?;

    for _ in 0..MAX_MOVES {
        let record = state.runs.get(&issue.key);
        let name = record
            .map_or(TRIGGERED, |record| record.pipeline.as_str())
            .to_owned();
        let pipeline = context.project.pipeline(&name);
        let run = record.map_or(Run::UNTRACKED, Record::run);
        let pending = record.and_then(|record| record.pending.first().copied());

        let requested = super::request::requested(requests, record, pipeline);

        let (event, why) = if let Some(requested) = requested {
            state.record(issue, &name).pending.clear();
            requested
        } else if let Some(action) = pending {
            let record = state.record(issue, &name);

            match perform(action, watched, &mut seen, record, pipeline, context) {
                Ok(next) => {
                    match (next, record.pending.first_mut()) {
                        (Some(next), Some(first)) => *first = next,
                        _ => {
                            record.pending.remove(0);
                        }
                    }

                    save(state)?;
                    continue;
                }
                Err(error) if error.is_permanent() => match (run.phase, action) {
                    (Phase::Claiming, _) => {
                        record.pending.clear();
                        (Event::ClaimFailed, None)
                    }
                    (_, Action::Start(step)) => {
                        record.pending.clear();
                        (Event::StartFailed { step }, Some(error.message))
                    }
                    _ => return Ok(Err(error)),
                },
                Err(error) => return Ok(Err(error)),
            }
        } else {
            match next_event(run, record, pipeline, watched, &seen, context) {
                Ok(Some(found)) => found,
                Ok(None) => break,
                Err(error) => return Ok(Err(error)),
            }
        };

        let fallback = context.project.pipelines.first();
        let Some(plan) = pipeline.or(fallback).map(Pipeline::plan) else {
            break;
        };
        let (next, actions) = step(run, event, plan);

        if next == run && actions.is_empty() {
            break;
        }

        let record = state.record(issue, &name);
        let what = what(event, record, pipeline);
        moved(record, (event, why), pipeline, next, actions);
        save(state)?;
        moves.push(Move {
            key: issue.key.clone(),
            pipeline: name,
            from: run.phase,
            to: next.phase,
            what,
        });
    }

    Ok(Ok(()))
}

/// Keeps a move: a request takes the pipeline's steps afresh, and a step's
/// event keeps why.
fn moved(
    record: &mut Record,
    (event, why): Found,
    pipeline: Option<&Pipeline>,
    next: Run,
    actions: Vec<Action>,
) {
    if let (Event::Requested, Some(pipeline)) = (event, pipeline) {
        record.begin(pipeline);
    }

    if let (Some(index), Some(why)) = (stepped(event), why)
        && let Some(kept) = record.steps.get_mut(usize::from(index))
    {
        kept.why = Some(why);
    }

    record.phase = next.phase;
    record.progress = next.progress;
    record.pending = actions;
}

/// The next event for a run with nothing pending: a claim confirms
/// itself, a running run hears its steps first and then its item, and a
/// run not working hears a request.
fn next_event(
    run: Run,
    record: Option<&Record>,
    pipeline: Option<&Pipeline>,
    watched: &Watched,
    seen: &Seen,
    context: &Context<'_, '_>,
) -> Result<Option<Found>, PluginError> {
    let found = |event| Ok(Some((event, None)));

    match (run.phase, record, pipeline) {
        (Phase::Claiming, _, Some(_)) => return found(Event::Claimed),
        (Phase::Running, Some(record), Some(pipeline)) => {
            let resumes = pipeline.plan().resume(record.progress).is_ok();

            if !resumes || !record.follows(pipeline) {
                return found(Event::Changed);
            }

            if let Some(event) = steps(record, pipeline, watched, seen, context)? {
                return Ok(Some(event));
            }
        }
        (Phase::Claiming | Phase::Running, _, _) => return found(Event::Changed),
        _ => {}
    }

    let requested = seen.open && seen.marks.has(crate::Mark::Build);
    let unmarked = !seen.marks.has(crate::Mark::Working);

    let event = match run.phase {
        Phase::Running if !seen.open => Event::IssueClosed,
        Phase::Running if unmarked => Event::Unmarked,
        Phase::Untracked | Phase::Blocked(_) | Phase::Stopped | Phase::Done if requested => {
            Event::Requested
        }
        Phase::Untracked if seen.open && !unmarked => Event::LostTrack,
        _ => return Ok(None),
    };

    found(event)
}

/// The first event a step's observation gives: a running step settled or
/// failed a round, or a settled restart step runs again.
fn steps(
    record: &Record,
    pipeline: &Pipeline,
    watched: &Watched,
    seen: &Seen,
    context: &Context<'_, '_>,
) -> Result<Option<Found>, PluginError> {
    let brief = brief(&watched.issue, record, context)?;

    for (index, spec) in pipeline.steps().iter().enumerate() {
        let settled = match record.progress.status.get(index) {
            Some(Status::Running) => false,
            Some(Status::Settled(outcome)) if spec.restart && *outcome != Outcome::FAILED => true,
            _ => continue,
        };
        let Ok(position) = u8::try_from(index) else {
            continue;
        };

        let plugin = context.plugins.step(spec.plugin)?;
        let places = Places::new(record, &watched.issue.key, spec, context);
        let run = places.context(index, settled, &brief, watched, seen, record, context);

        match (settled, plugin.observe(&run, spec)?) {
            (false, Observed::Settled(outcome)) => {
                let Some(number) = spec
                    .outcome(&outcome.name)
                    .filter(|o| *o != Outcome::FAILED)
                else {
                    return Err(PluginError::permanent(format!(
                        "{} settled {}, which is none of its outcomes",
                        spec.id, outcome.name
                    )));
                };
                let event = Event::Settled {
                    step: position,
                    outcome: number,
                };

                return Ok(Some((event, Some(outcome.why))));
            }
            (false, Observed::Failed { why, last }) => {
                let event = Event::Failed {
                    step: position,
                    last,
                };

                return Ok(Some((event, Some(why))));
            }
            (true, Observed::Restarted) => {
                return Ok(Some((Event::Pushed { step: position }, None)));
            }
            _ => {}
        }
    }

    Ok(None)
}

/// The names and folders a step's context borrows.
struct Places {
    name: String,
    folder: PathBuf,
}

impl Places {
    fn new(
        record: &Record,
        key: &IssueKey,
        spec: &crate::StepSpec,
        context: &Context<'_, '_>,
    ) -> Self {
        let name = Record::folder(key);
        let folder = context
            .project
            .steps
            .join(&name)
            .join(&record.pipeline)
            .join(record.generation.to_string())
            .join(&spec.id);

        Self { name, folder }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "a context is all of these, borrowed"
    )]
    fn context<'a>(
        &'a self,
        index: usize,
        settled: bool,
        brief: &'a Brief,
        watched: &'a Watched,
        seen: &Seen,
        record: &'a Record,
        context: &'a Context<'_, '_>,
    ) -> RunContext<'a> {
        let project = context.project;

        RunContext {
            repo: &project.repo,
            local: &project.local,
            base: &project.base,
            issue: &watched.issue,
            name: &self.name,
            branch: &record.branch,
            worktree: record.worktree.as_deref(),
            folder: &self.folder,
            brief,
            session: record
                .steps
                .get(index)
                .and_then(|step| step.session.as_ref()),
            pull_request: seen.pull_request,
            failed: record.progress.failed.get(index).copied().unwrap_or(0),
            settled,
        }
    }
}

/// The builder's brief for this item: core's contract with the tracker's
/// and the forge's parts.
fn brief(
    issue: &crate::Issue,
    record: &Record,
    context: &Context<'_, '_>,
) -> Result<Brief, PluginError> {
    let Context { project, plugins } = context;
    let tracker = tracker(plugins, &issue.key)?;

    Ok(Brief::new(
        &tracker.reference(issue),
        issue,
        &record.branch,
        &project.base,
    )
    .with(tracker.brief(&project.repo, issue))
    .with(
        plugins
            .forge
            .brief(&project.repo, &project.base, &record.branch),
    )
    .deny(plugins.forge.denied()))
}

/// The step an event names, if any.
const fn stepped(event: Event) -> Option<u8> {
    match event {
        Event::Settled { step, .. } | Event::Failed { step, .. } | Event::StartFailed { step } => {
            Some(step)
        }
        _ => None,
    }
}

/// The event as a person reads it in the log, such as `builder settled
/// opened`.
fn what(event: Event, record: &Record, pipeline: Option<&Pipeline>) -> String {
    let id = |step| record.step_id(step);

    match event {
        Event::Requested => "requested".to_owned(),
        Event::Claimed => "claimed".to_owned(),
        Event::ClaimFailed => "could not claim it".to_owned(),
        Event::Settled { step, outcome } => {
            let spec = pipeline.and_then(|pipeline| pipeline.steps().get(usize::from(step)));
            let name = spec
                .and_then(|spec| spec.outcome_name(outcome))
                .map_or_else(|| outcome.0.to_string(), str::to_owned);

            format!("{} settled {name}", id(step))
        }
        Event::Failed { step, last: false } => format!("{} failed a round", id(step)),
        Event::Failed { step, last: true } => format!("{} failed", id(step)),
        Event::StartFailed { step } => format!("{} could not start", id(step)),
        Event::Pushed { step } => format!("{} runs again", id(step)),
        Event::Changed => "its pipeline changed".to_owned(),
        Event::Stop => "stop requested".to_owned(),
        Event::Unmarked => "lost its working mark".to_owned(),
        Event::IssueClosed => "the issue closed".to_owned(),
        Event::LostTrack => "carries the working mark with no run".to_owned(),
    }
}

/// Keeps the number of the pull request the poll saw, so `status` shows it
/// without asking the forge.
fn remember_pull_request<E>(
    watched: &Watched,
    state: &mut State,
    save: &mut impl FnMut(&State) -> Result<(), E>,
) -> Result<(), E> {
    let Some(number) = watched.seen.pull_request.map(PullRequest::number) else {
        return Ok(());
    };
    let Some(record) = state.runs.get_mut(&watched.issue.key) else {
        return Ok(());
    };

    if record.pull_request == Some(number) {
        return Ok(());
    }

    record.pull_request = Some(number);
    save(state)
}

/// Carries out one action; an action that leaves another in its place,
/// as a start that made a session leaves its announcement, returns it.
fn perform(
    action: Action,
    watched: &Watched,
    seen: &mut Seen,
    record: &mut Record,
    pipeline: Option<&Pipeline>,
    context: &Context<'_, '_>,
) -> Result<Option<Action>, PluginError> {
    let Context { project, plugins } = context;
    let repo = &project.repo;
    let issue = &watched.issue;
    let tracker = tracker(plugins, &issue.key)?;

    match action {
        Action::Mark(mark) => {
            tracker.mark(repo, &issue.key.id, mark)?;
            seen.marks = seen.marks.with(mark);
        }
        Action::Unmark(mark) => {
            tracker.unmark(repo, &issue.key.id, mark)?;
            seen.marks = seen.marks.without(mark);
        }
        Action::CloseIssue => {
            tracker.close(repo, &issue.key.id)?;
            seen.open = false;
        }
        Action::Announce(index) => {
            let session = record
                .steps
                .get(usize::from(index))
                .and_then(|step| step.session.as_ref())
                .map_or("unknown", |session| &session.id);
            let text = format!(
                "atelier started `{}`: session `{session}` on branch `{}`.",
                record.step_id(index),
                record.branch
            );

            tracker.comment(repo, &issue.key.id, &text)?;
        }
        Action::Comment(reason) => {
            tracker.comment(repo, &issue.key.id, &blocked(reason, record))?;
        }
        Action::Report => {
            tracker.comment(repo, &issue.key.id, &report(record, pipeline))?;
        }
        Action::Start(index) => return start(index, watched, seen, record, pipeline, context),
        Action::Interrupt => {
            let why = stopped(record);

            for session in record.sessions() {
                plugins.agent.interrupt(session)?;
                plugins.agent.tell(session, &why)?;
            }

            cleanup(record, &issue.key, context)?;
        }
        Action::RemoveWorktree => {
            if let Some(worktree) = record.worktree.clone() {
                plugins.worktrees.remove(&project.local, &worktree)?;
                record.worktree = None;
            }
        }
    }

    Ok(None)
}

/// Releases each started step's resources, even when its pipeline was
/// removed: the record keeps which plugin started it.
fn cleanup(record: &Record, key: &IssueKey, context: &Context<'_, '_>) -> Result<(), PluginError> {
    let root = context
        .project
        .steps
        .join(Record::folder(key))
        .join(&record.pipeline)
        .join(record.generation.to_string());

    for (index, kept) in record.steps.iter().enumerate() {
        if record.progress.status.get(index) == Some(&Status::Pending) {
            continue;
        }

        let plugin = if kept.plugin.is_empty() {
            context
                .project
                .pipeline(&record.pipeline)
                .and_then(|pipeline| pipeline.steps().get(index))
                .map(|spec| spec.plugin)
        } else {
            Some(kept.plugin.as_str())
        };

        if let Some(plugin) = plugin {
            context.plugins.step(plugin)?.stop(&root.join(&kept.id))?;
        }
    }

    Ok(())
}

/// Starts step `index` through its plugin, and keeps what the start made.
fn start(
    index: u8,
    watched: &Watched,
    seen: &Seen,
    record: &mut Record,
    pipeline: Option<&Pipeline>,
    context: &Context<'_, '_>,
) -> Result<Option<Action>, PluginError> {
    let position = usize::from(index);
    let spec = pipeline
        .and_then(|pipeline| pipeline.steps().get(position))
        .ok_or_else(|| {
            PluginError::permanent(format!("{} has no step {index}", record.pipeline))
        })?;
    let plugin = context.plugins.step(spec.plugin)?;
    let brief = brief(&watched.issue, record, context)?;
    let places = Places::new(record, &watched.issue.key, spec, context);
    let run = places.context(position, false, &brief, watched, seen, record, context);

    match plugin.start(&run, spec)? {
        Started::Nothing => {}
        Started::Worktree(worktree) => record.worktree = Some(worktree),
        Started::Session(session) => {
            if let Some(kept) = record.steps.get_mut(position) {
                kept.session = Some(session);
            }

            return Ok(Some(Action::Announce(index)));
        }
        Started::Tell(text) => {
            for session in record.sessions() {
                context.plugins.agent.tell(session, &text)?;
            }
        }
    }

    Ok(None)
}

fn tracker<'a>(plugins: &Plugins<'a>, key: &IssueKey) -> Result<&'a dyn Tracker, PluginError> {
    plugins
        .source(&key.tracker)
        .map(|source| source.tracker)
        .ok_or_else(|| {
            PluginError::permanent(format!(
                "{key}: this project has no tracker {}",
                key.tracker
            ))
        })
}

/// What an interrupted resident step hears: why, and to act on nothing
/// later, since its pull request's events still wake it.
fn stopped(record: &Record) -> String {
    let why = match record.phase {
        Phase::Blocked(reason) => blocked(reason, record),
        Phase::Done => format!("atelier finished this run of {}.", record.pipeline),
        _ => "atelier stopped this run: it was stopped, lost its working mark or was closed."
            .to_owned(),
    };

    format!(
        "{why} Take no further action on this issue: when a message arrives later, \
         such as a failed check or a review, end your turn without acting. If the \
         issue is requested again, a new session takes over."
    )
}

fn blocked(reason: Reason, record: &Record) -> String {
    let why = |step: u8| {
        record
            .steps
            .get(usize::from(step))
            .and_then(|kept| kept.why.as_deref())
            .map_or_else(String::new, |why| format!(": {why}"))
    };

    match reason {
        Reason::Claim => "atelier could not claim this issue; its log says why. Add the build \
                          mark again to retry."
            .to_owned(),
        Reason::Start(step) => format!(
            "atelier could not start `{}` for this issue{}. Add the build mark again to retry.",
            record.step_id(step),
            why(step)
        ),
        Reason::Failed(step) => format!(
            "atelier stopped this run: `{}` failed{}.",
            record.step_id(step),
            why(step)
        ),
        Reason::Changed => "atelier stopped this run: its pipeline changed while it ran. Add \
                            the build mark again to start it over."
            .to_owned(),
        Reason::LostTrack => "atelier holds no record of this issue, though it carries the \
                              working mark. Add the build mark to start it again."
            .to_owned(),
    }
}

/// Each step's outcome, as the comment that closes a run lists them.
fn report(record: &Record, pipeline: Option<&Pipeline>) -> String {
    let mut lines = vec![
        format!("atelier finished `{}`:", record.pipeline),
        String::new(),
    ];

    for (index, kept) in record.steps.iter().enumerate() {
        let spec = pipeline.and_then(|pipeline| pipeline.steps().get(index));
        let status = match record.progress.status.get(index) {
            Some(Status::Settled(outcome)) => spec
                .and_then(|spec| spec.outcome_name(*outcome))
                .map_or_else(|| outcome.0.to_string(), str::to_owned),
            Some(Status::Skipped) => "skipped".to_owned(),
            Some(Status::Running) => "running".to_owned(),
            Some(Status::Pending) | None => "not run".to_owned(),
        };
        let why = kept
            .why
            .as_deref()
            .map_or_else(String::new, |why| format!(" ({why})"));

        lines.push(format!("- `{}`: {status}{why}", kept.id));
    }

    lines.join("\n")
}
