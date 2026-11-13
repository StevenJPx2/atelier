use rungs::{Failed, Plan, Progress, State, Status};

use super::{Action, Event, Mark, Phase, Reason, Run};

/// Moves one run on one event, over `plan`, and names the actions that
/// carry the move out. Total: an event that does not apply leaves the run
/// and names nothing.
///
/// ```
/// use atelier_core::rungs::{Need, Outcome, Plan, Step};
/// use atelier_core::{Action, Event, Phase, Run, step};
///
/// // worktree, then deps and audit side by side
/// let plan = Plan::new([
///     Step::new(1),
///     Step::new(1).after([Need::outcome(0, [Outcome(0)])]),
///     Step::new(2).after([Need::outcome(0, [Outcome(0)])]),
/// ])?;
/// let (claiming, _) = step(Run::UNTRACKED, Event::Requested, &plan);
/// let (running, _) = step(claiming, Event::Claimed, &plan);
///
/// let worktree = Event::Settled { step: 0, outcome: Outcome(0) };
/// let (next, actions) = step(running, worktree, &plan);
///
/// assert_eq!(next.phase, Phase::Running);
/// assert_eq!(actions, [Action::Start(1), Action::Start(2)]);
/// # Ok::<(), atelier_core::rungs::PlanError>(())
/// ```
#[must_use]
pub fn step(run: Run, event: Event, plan: &Plan) -> (Run, Vec<Action>) {
    outside(run, event)
        .or_else(|| inside(run, event, plan))
        .unwrap_or((run, Vec::new()))
}

/// A move rungs plays no part in; none when the event asks rungs to start
/// the run or move one of its steps.
pub(super) fn outside(run: Run, event: Event) -> Option<(Run, Vec<Action>)> {
    let working = run.phase.is_working();

    let moved = match (run.phase, event) {
        (Phase::Untracked | Phase::Stopped | Phase::Done, Event::Requested) => claim(None),
        (Phase::Blocked(_), Event::Requested) => claim(Some(Action::Unmark(Mark::Blocked))),
        (Phase::Claiming, Event::ClaimFailed) => block(run, Reason::Claim),
        (_, Event::Changed) if working => block(run, Reason::Changed),
        (_, Event::Stop) if working => {
            let (stopped, mut actions) = stop(run);
            actions.insert(0, Action::Unmark(Mark::Build));
            (stopped, actions)
        }
        (_, Event::Unmarked | Event::IssueClosed) if working => stop(run),
        (Phase::Untracked, Event::LostTrack) => block(run, Reason::LostTrack),
        (Phase::Claiming, Event::Claimed)
        | (
            Phase::Running,
            Event::Settled { .. }
            | Event::Failed { .. }
            | Event::StartFailed { .. }
            | Event::Pushed { .. },
        ) => return None,
        _ => (run, Vec::new()),
    };

    Some(moved)
}

/// A claimed run starts its first steps; a running one moves a step.
fn inside(run: Run, event: Event, plan: &Plan) -> Option<(Run, Vec<Action>)> {
    if run.phase == Phase::Claiming {
        return Some(advance(plan.start(), Vec::new()));
    }

    running(run, event, plan)
}

/// Claiming, with fresh progress: the working mark comes and the build
/// mark goes.
fn claim(also: Option<Action>) -> (Run, Vec<Action>) {
    let mut actions = vec![Action::Mark(Mark::Working), Action::Unmark(Mark::Build)];
    actions.extend(also);

    let run = Run {
        phase: Phase::Claiming,
        progress: Progress::default(),
    };

    (run, actions)
}

/// A running run on one of its steps' events; none when it does not
/// apply.
fn running(run: Run, event: Event, plan: &Plan) -> Option<(Run, Vec<Action>)> {
    let mut steps = plan.resume(run.progress).ok()?;

    match event {
        Event::Settled { step, outcome } => steps.settle(usize::from(step), outcome).ok()?,
        Event::Failed { step, last } => {
            let mut failed = steps.fail(usize::from(step)).ok()?;

            while last && failed != Failed::Final {
                failed = steps.fail(usize::from(step)).ok()?;
            }

            if let Failed::Retry(_) = failed {
                let run = Run {
                    phase: Phase::Running,
                    progress: steps.progress(),
                };

                return Some((run, vec![Action::Start(step)]));
            }
        }
        Event::StartFailed { step } if steps.status(usize::from(step)) == Some(Status::Running) => {
            return Some(block(run, Reason::Start(step)));
        }
        Event::Pushed { step }
            if matches!(steps.status(usize::from(step)), Some(Status::Settled(_))) =>
        {
            steps.restart(usize::from(step)).ok()?;
        }
        _ => return None,
    }

    Some(advance(steps, Vec::new()))
}

/// Starts each step whose needs now hold, after `actions`; Done or
/// Blocked once no step runs and none can start.
fn advance(mut steps: rungs::Run<'_>, actions: Vec<Action>) -> (Run, Vec<Action>) {
    let started = steps.advance();
    let started = core::array::from_fn(|index| started.contains(index));

    conclude(started, steps.state(), steps.progress(), actions)
}

/// The run once rungs has moved it to `progress`: each step in `started`
/// starts, after `actions`, and the run is Done or Blocked once rungs'
/// `state` says it is over.
pub(super) fn conclude(
    started: [bool; rungs::MAX_STEPS],
    state: State,
    progress: Progress,
    mut actions: Vec<Action>,
) -> (Run, Vec<Action>) {
    actions.reserve(rungs::MAX_STEPS.saturating_add(DONE.len()));

    for (index, started) in started.into_iter().enumerate() {
        if let (true, Some(step)) = (started, position(index)) {
            actions.push(Action::Start(step));
        }
    }

    let run = Run {
        phase: Phase::Running,
        progress,
    };

    match state {
        State::Running => (run, actions),
        State::Over { unhandled: None } => {
            actions.extend(DONE);

            let done = Run {
                phase: Phase::Done,
                ..run
            };

            (done, actions)
        }
        State::Over {
            unhandled: Some(step),
        } => {
            let reason = Reason::Failed(position(step).unwrap_or(u8::MAX));
            let (blocked, more) = block(run, reason);
            actions.extend(more);

            (blocked, actions)
        }
    }
}

/// What a run that is done does: its resident steps hear it, each step's
/// outcome is reported, the issue closes, and the working mark and the
/// worktree go.
const DONE: [Action; 5] = [
    Action::Interrupt,
    Action::Report,
    Action::CloseIssue,
    Action::Unmark(Mark::Working),
    Action::RemoveWorktree,
];

fn position(step: usize) -> Option<u8> {
    u8::try_from(step).ok()
}

/// Blocked: a running run's resident steps are interrupted, the working
/// mark goes, the blocked mark comes, a comment says why, and the worktree
/// goes. A claim that failed takes the build mark too, so it is not
/// requested again until a person asks.
fn block(run: Run, reason: Reason) -> (Run, Vec<Action>) {
    let ran = run.phase == Phase::Running;
    let mut actions = Vec::new();

    if ran {
        actions.push(Action::Interrupt);
    }

    if reason == Reason::Claim {
        actions.push(Action::Unmark(Mark::Build));
    }

    actions.extend([
        Action::Unmark(Mark::Working),
        Action::Mark(Mark::Blocked),
        Action::Comment(reason),
    ]);

    if ran {
        actions.push(Action::RemoveWorktree);
    }

    let blocked = Run {
        phase: Phase::Blocked(reason),
        ..run
    };

    (blocked, actions)
}

/// Stopped, when the item lost its working mark or closed.
fn stop(run: Run) -> (Run, Vec<Action>) {
    let actions = if run.phase == Phase::Running {
        vec![
            Action::Interrupt,
            Action::Unmark(Mark::Working),
            Action::RemoveWorktree,
        ]
    } else {
        vec![Action::Unmark(Mark::Working)]
    };

    let stopped = Run {
        phase: Phase::Stopped,
        ..run
    };

    (stopped, actions)
}

#[cfg(test)]
mod tests {
    use rungs::{Need, Outcome, Step};

    use super::*;

    const PASSED: Outcome = Outcome(0);
    const NOTHING: Outcome = Outcome(1);

    const WORKTREE: u8 = 0;
    const BUILDER: u8 = 1;
    const CHECKS: u8 = 2;
    const READY: u8 = 3;
    const MERGE: u8 = 4;

    /// The built-in build: worktree, builder (opened or nothing), checks
    /// after the builder opened, with three rounds, then ready and merge.
    fn build() -> Plan {
        let after = |step| [Need::outcome(step, [Outcome(0)])];

        Plan::new([
            Step::new(1),
            Step::new(2).after(after(0)),
            Step::new(1).attempts(3).after(after(1)),
            Step::new(1).after(after(2)),
            Step::new(1).after(after(3)),
        ])
        .unwrap()
    }

    /// A run of the build, moved by `events` from untracked.
    fn moved(events: &[Event]) -> Run {
        events
            .iter()
            .fold(Run::UNTRACKED, |run, event| step(run, *event, &build()).0)
    }

    fn settled(step: u8, outcome: Outcome) -> Event {
        Event::Settled { step, outcome }
    }

    fn building() -> Run {
        moved(&[Event::Requested, Event::Claimed, settled(WORKTREE, PASSED)])
    }

    fn checking() -> Run {
        let built = step(building(), settled(BUILDER, PASSED), &build()).0;
        assert_eq!(built.progress.status[2], Status::Running);
        built
    }

    fn status(run: Run, step: u8) -> Status {
        run.progress.status[usize::from(step)]
    }

    #[test]
    fn a_request_claims_then_starts_each_step_as_its_needs_hold() {
        let plan = build();
        let blocked = Run {
            phase: Phase::Blocked(Reason::Claim),
            ..Run::UNTRACKED
        };

        let (claiming, claim) = step(Run::UNTRACKED, Event::Requested, &plan);
        let (_, reclaim) = step(blocked, Event::Requested, &plan);
        let (running, start) = step(claiming, Event::Claimed, &plan);
        let (built, builder) = step(running, settled(WORKTREE, PASSED), &plan);

        assert_eq!(claiming.phase, Phase::Claiming);
        assert_eq!(
            claim,
            [Action::Mark(Mark::Working), Action::Unmark(Mark::Build)]
        );
        assert_eq!(reclaim.last(), Some(&Action::Unmark(Mark::Blocked)));
        assert_eq!(running.phase, Phase::Running);
        assert_eq!(start, [Action::Start(WORKTREE)]);
        assert_eq!(builder, [Action::Start(BUILDER)]);
        assert_eq!(status(built, BUILDER), Status::Running);
    }

    #[test]
    fn failed_rounds_start_a_step_again_until_the_last_blocks_the_run() {
        let plan = build();
        let fail = Event::Failed {
            step: CHECKS,
            last: false,
        };

        let (once, again) = step(checking(), fail, &plan);
        let (twice, _) = step(once, fail, &plan);
        let (blocked, actions) = step(twice, fail, &plan);

        assert_eq!(again, [Action::Start(CHECKS)]);
        assert_eq!(once.progress.failed[2], 1);
        assert_eq!(blocked.phase, Phase::Blocked(Reason::Failed(CHECKS)));
        assert_eq!(
            actions,
            [
                Action::Interrupt,
                Action::Unmark(Mark::Working),
                Action::Mark(Mark::Blocked),
                Action::Comment(Reason::Failed(CHECKS)),
                Action::RemoveWorktree,
            ]
        );
    }

    #[test]
    fn a_last_failure_is_final_at_once() {
        let last = Event::Failed {
            step: CHECKS,
            last: true,
        };

        let (blocked, _) = step(checking(), last, &build());

        assert_eq!(blocked.phase, Phase::Blocked(Reason::Failed(CHECKS)));
        assert_eq!(blocked.progress.failed[2], 3);
    }

    #[test]
    fn a_push_sends_the_run_back_keeping_failed_rounds() {
        let plan = build();
        let fail = Event::Failed {
            step: CHECKS,
            last: false,
        };
        let failed_once = step(checking(), fail, &plan).0;
        let passed = step(failed_once, settled(CHECKS, PASSED), &plan).0;
        let ready = step(passed, settled(READY, PASSED), &plan).0;

        let (pushed, actions) = step(ready, Event::Pushed { step: CHECKS }, &plan);

        assert_eq!(actions, [Action::Start(CHECKS)]);
        assert_eq!(status(pushed, CHECKS), Status::Running);
        assert_eq!(status(pushed, READY), Status::Pending);
        assert_eq!(status(pushed, MERGE), Status::Pending);
        assert_eq!(pushed.progress.failed[2], 1);
        assert_eq!(status(pushed, BUILDER), Status::Settled(PASSED));
        let running = checking();
        assert_eq!(
            step(running, Event::Pushed { step: CHECKS }, &plan),
            (running, vec![])
        );
        assert_eq!(
            step(building(), Event::Pushed { step: CHECKS }, &plan).1,
            []
        );
    }

    #[test]
    fn a_merge_is_done_and_nothing_left_to_run_is_done_too() {
        let plan = build();
        let passed = step(checking(), settled(CHECKS, PASSED), &plan).0;
        let ready = step(passed, settled(READY, PASSED), &plan).0;

        let (done, actions) = step(ready, settled(MERGE, PASSED), &plan);
        let (nothing, _) = step(building(), settled(BUILDER, NOTHING), &plan);

        assert_eq!(done.phase, Phase::Done);
        assert_eq!(
            actions,
            [
                Action::Interrupt,
                Action::Report,
                Action::CloseIssue,
                Action::Unmark(Mark::Working),
                Action::RemoveWorktree,
            ]
        );
        assert_eq!(nothing.phase, Phase::Done);
        assert_eq!(status(nothing, CHECKS), Status::Skipped);
    }

    #[test]
    fn a_step_that_cannot_start_blocks_the_run() {
        let plan = build();

        let (blocked, actions) = step(building(), Event::StartFailed { step: BUILDER }, &plan);
        let (claim, unclaim) = step(moved(&[Event::Requested]), Event::ClaimFailed, &plan);
        let (same, none) = step(building(), Event::StartFailed { step: CHECKS }, &plan);

        assert_eq!(blocked.phase, Phase::Blocked(Reason::Start(BUILDER)));
        assert_eq!(actions.first(), Some(&Action::Interrupt));
        assert_eq!(claim.phase, Phase::Blocked(Reason::Claim));
        assert_eq!(
            unclaim,
            [
                Action::Unmark(Mark::Build),
                Action::Unmark(Mark::Working),
                Action::Mark(Mark::Blocked),
                Action::Comment(Reason::Claim),
            ]
        );
        assert_eq!((same, none), (building(), vec![]));
    }

    #[test]
    fn losing_the_mark_or_the_issue_stops_the_run() {
        let plan = build();

        let (stopped, actions) = step(building(), Event::Unmarked, &plan);
        let (closed, unmark) = step(moved(&[Event::Requested]), Event::IssueClosed, &plan);
        let (changed, _) = step(building(), Event::Changed, &plan);
        let (lost, _) = step(Run::UNTRACKED, Event::LostTrack, &plan);

        assert_eq!(stopped.phase, Phase::Stopped);
        assert_eq!(
            actions,
            [
                Action::Interrupt,
                Action::Unmark(Mark::Working),
                Action::RemoveWorktree,
            ]
        );
        assert_eq!(closed.phase, Phase::Stopped);
        assert_eq!(unmark, [Action::Unmark(Mark::Working)]);
        assert_eq!(changed.phase, Phase::Blocked(Reason::Changed));
        assert_eq!(lost.phase, Phase::Blocked(Reason::LostTrack));
    }

    #[test]
    fn an_event_that_does_not_apply_changes_nothing() {
        let plan = build();
        let done = Run {
            phase: Phase::Done,
            ..building()
        };

        let cases = [
            (building(), Event::Claimed),
            (building(), settled(CHECKS, PASSED)),
            (building(), settled(BUILDER, Outcome(5))),
            (Run::UNTRACKED, Event::Unmarked),
            (Run::UNTRACKED, Event::Stop),
            (done, Event::Stop),
            (done, Event::Changed),
            (done, settled(BUILDER, PASSED)),
            (Run::UNTRACKED, Event::Claimed),
            (building(), Event::LostTrack),
        ];

        for (run, event) in cases {
            assert_eq!(step(run, event, &plan), (run, vec![]), "{event:?}");
        }
    }
}
