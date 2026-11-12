use super::{Observation, Observed, Plugins, Project, Record, State};
use crate::{
    Action, Brief, Event, IssueKey, Phase, PluginError, PullRequest, Reason, Seen, Tracker,
    WorktreeRequest, events, step,
};

/// The most steps one issue takes in one poll. A start takes four and a
/// merge one, so this only stops a fault from looping.
const MAX_STEPS: usize = 32;

/// What one poll did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub steps: Vec<Step>,
    pub failures: Vec<(IssueKey, PluginError)>,
}

/// One issue's move from one phase to the next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub key: IssueKey,
    pub from: Phase,
    pub to: Phase,
}

struct Context<'p, 'a> {
    project: &'p Project,
    plugins: &'p Plugins<'a>,
}

/// Steps every observed issue and carries out its actions through the
/// plugins.
///
/// `save` runs after each step and each action, so a stop between
/// actions loses nothing; the next poll finishes what is pending. One
/// issue's failure is reported and leaves the others to go on. The trackers'
/// listing tags are kept only when every issue succeeded, so an issue that
/// failed is listed again.
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

    for (key, observed) in &observation.issues {
        if let Err(error) = drive(observed, state, &context, save, &mut report.steps)? {
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
    observed: &Observed,
    state: &mut State,
    context: &Context<'_, '_>,
    save: &mut impl FnMut(&State) -> Result<(), E>,
    steps: &mut Vec<Step>,
) -> Result<Result<(), PluginError>, E> {
    let key = &observed.issue.key;
    let mut seen = observed.seen;

    remember_pull_request(observed, state, save)?;

    for _ in 0..MAX_STEPS {
        let record = state.issues.get(key);
        let phase = record.map_or(Phase::Untracked, |record| record.phase);
        let pending = record.and_then(|record| record.pending.first().copied());

        let event = if let Some(action) = pending {
            let record = state.record(&observed.issue);

            match perform(action, observed, &mut seen, record, context) {
                Ok(()) => {
                    record.pending.remove(0);
                    save(state)?;
                    continue;
                }
                Err(error) if error.is_permanent() && starting(phase) => {
                    record.pending.clear();
                    Event::StartFailed
                }
                Err(error) => return Ok(Err(error)),
            }
        } else if let Some(event) =
            confirmation(phase).or_else(|| events(phase, &seen).first().copied())
        {
            event
        } else {
            break;
        };

        let (next, actions) = step(phase, event, &context.project.pipeline);

        if next == phase && actions.is_empty() {
            break;
        }

        let record = state.record(&observed.issue);
        record.phase = next;
        record.pending = actions;
        save(state)?;
        steps.push(Step {
            key: key.clone(),
            from: phase,
            to: next,
        });
    }

    Ok(Ok(()))
}

/// Keeps the number of the pull request the poll saw, so `status` shows it
/// without asking the forge.
fn remember_pull_request<E>(
    observed: &Observed,
    state: &mut State,
    save: &mut impl FnMut(&State) -> Result<(), E>,
) -> Result<(), E> {
    let Some(number) = observed.seen.pull_request.map(PullRequest::number) else {
        return Ok(());
    };
    let Some(record) = state.issues.get_mut(&observed.issue.key) else {
        return Ok(());
    };

    if record.pull_request == Some(number) {
        return Ok(());
    }

    record.pull_request = Some(number);
    save(state)
}

const fn starting(phase: Phase) -> bool {
    matches!(phase, Phase::Claiming | Phase::Preparing | Phase::Starting)
}

/// A start phase whose action is done confirms itself.
const fn confirmation(phase: Phase) -> Option<Event> {
    match phase {
        Phase::Claiming => Some(Event::Claimed),
        Phase::Preparing => Some(Event::WorktreeReady),
        Phase::Starting => Some(Event::AgentStarted),
        _ => None,
    }
}

fn perform(
    action: Action,
    observed: &Observed,
    seen: &mut Seen,
    record: &mut Record,
    context: &Context<'_, '_>,
) -> Result<(), PluginError> {
    let repo = &context.project.repo;
    let issue = &observed.issue;
    let tracker = tracker(context.plugins, &issue.key)?;

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
        Action::CommentSession => {
            let session = record
                .session
                .as_ref()
                .map_or("unknown", |session| &session.id);
            let text = format!(
                "atelier started a builder: session `{session}` on branch `{}`.",
                record.branch
            );

            tracker.comment(repo, &issue.key.id, &text)?;
        }
        Action::Comment(reason) => {
            tracker.comment(repo, &issue.key.id, &blocked(reason, context.project))?;
        }
        Action::CreateWorktree | Action::StartBuilder | Action::RemoveWorktree => {
            build(action, observed, tracker, record, context)?;
        }
        Action::Interrupt => {
            if let Some(session) = &record.session {
                context.plugins.agent.interrupt(session)?;
            }
        }
        Action::StartStage(index) => {
            let name = context
                .project
                .pipeline
                .stage_name(index)
                .unwrap_or("unknown");

            return Err(PluginError::permanent(format!(
                "no plugin runs the stage {name}"
            )));
        }
    }

    Ok(())
}

fn build(
    action: Action,
    observed: &Observed,
    tracker: &dyn Tracker,
    record: &mut Record,
    context: &Context<'_, '_>,
) -> Result<(), PluginError> {
    let Context { project, plugins } = context;
    let issue = &observed.issue;

    match (action, record.worktree.clone()) {
        (Action::CreateWorktree, _) => {
            let request = WorktreeRequest {
                repo: &project.repo,
                checkout: &project.checkout,
                name: &Record::folder(&issue.key),
                branch: &record.branch,
                base: &project.base,
            };

            record.worktree = Some(plugins.worktrees.create(&request)?);
        }
        (Action::StartBuilder, Some(worktree)) => {
            let brief = Brief::new(
                &tracker.reference(issue),
                issue,
                &record.branch,
                &project.base,
                &project.pipeline,
            )
            .with(tracker.brief(&project.repo, issue))
            .with(
                plugins
                    .forge
                    .brief(&project.repo, &project.base, &record.branch),
            )
            .deny(plugins.forge.denied());

            record.session = Some(plugins.agent.start(&worktree, &brief)?);
        }
        (Action::StartBuilder, None) => {
            return Err(PluginError::permanent("the builder has no worktree"));
        }
        (Action::RemoveWorktree, Some(worktree)) => {
            plugins.worktrees.remove(&project.checkout, &worktree)?;
            record.worktree = None;
        }
        _ => {}
    }

    Ok(())
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

fn blocked(reason: Reason, project: &Project) -> String {
    match reason {
        Reason::Start => "atelier could not start a builder for this issue; its log says why. \
                          Add the build mark again to retry."
            .to_owned(),
        Reason::RedRounds => format!(
            "atelier stopped the builder: CI failed {} rounds.",
            project.pipeline.red_rounds()
        ),
        Reason::Stage(index) => format!(
            "atelier stopped the builder: the {} stage failed its last round.",
            project.pipeline.stage_name(index).unwrap_or("unknown")
        ),
        Reason::Closed => {
            "atelier stopped the builder: its pull request was closed without merging.".to_owned()
        }
        Reason::LostTrack => "atelier holds no record of this issue, though it carries the \
                              working mark. Add the build mark to start it again."
            .to_owned(),
    }
}
