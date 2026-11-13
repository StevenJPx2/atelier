use super::{Action, Event, Limits, Mark, Phase, Reason};

/// Moves one issue's phase on one event, and names the actions that carry
/// the move out. Total: an event that does not apply leaves the phase and
/// names nothing.
///
/// ```
/// use atelier_core::{Action, Event, Mark, Phase, Limits, Reason, step};
///
/// let limits = Limits::new(5, [("critic", 2)])?;
///
/// let (next, actions) = step(Phase::Open { rounds: 4 }, Event::RoundFailed, &limits);
///
/// assert_eq!(next, Phase::Blocked(Reason::RedRounds));
/// assert_eq!(actions, [
///     Action::Interrupt,
///     Action::Unmark(Mark::Working),
///     Action::Mark(Mark::Blocked),
///     Action::Comment(Reason::RedRounds),
/// ]);
/// # Ok::<(), atelier_core::LimitsError>(())
/// ```
#[must_use]
pub fn step(phase: Phase, event: Event, limits: &Limits) -> (Phase, Vec<Action>) {
    match (phase, event) {
        (Phase::Untracked | Phase::Stopped, Event::Requested) => (
            Phase::Claiming,
            vec![Action::Mark(Mark::Working), Action::Unmark(Mark::Build)],
        ),
        (Phase::Blocked(_), Event::Requested) => (
            Phase::Claiming,
            vec![
                Action::Mark(Mark::Working),
                Action::Unmark(Mark::Build),
                Action::Unmark(Mark::Blocked),
            ],
        ),
        (Phase::Claiming, Event::Claimed) => (Phase::Preparing, vec![Action::CreateWorktree]),
        (Phase::Preparing, Event::WorktreeReady) => (Phase::Starting, vec![Action::StartBuilder]),
        (Phase::Starting, Event::AgentStarted) => (Phase::Building, vec![Action::CommentSession]),
        (Phase::Claiming | Phase::Preparing | Phase::Starting, Event::StartFailed) => {
            block(Reason::Start, vec![])
        }
        (Phase::Building, Event::PullRequestOpened) => (Phase::Open { rounds: 0 }, vec![]),
        (p, Event::Pushed) if p.has_pull_request() => (Phase::Open { rounds: p.rounds() }, vec![]),
        (Phase::Open { rounds }, Event::RoundFailed) => {
            let rounds = rounds.saturating_add(1);

            if rounds >= limits.red_rounds() {
                block(Reason::RedRounds, vec![Action::Interrupt])
            } else {
                (Phase::Open { rounds }, vec![])
            }
        }
        (Phase::Open { rounds }, Event::AllPassed) => stage(limits, 0, rounds),
        (Phase::Stage { index, rounds }, Event::StagePassed) => {
            stage(limits, index.saturating_add(1), rounds)
        }
        (Phase::Stage { index, .. }, Event::StageFailed { rounds })
            if limits.limit(index).is_some_and(|limit| rounds >= limit) =>
        {
            block(Reason::Stage(index), vec![Action::Interrupt])
        }
        (Phase::Green { rounds }, Event::MarkedReady) => (Phase::Ready { rounds }, vec![]),
        (p, Event::Merged) if p.has_pull_request() => (
            Phase::Done,
            vec![
                Action::CloseIssue,
                Action::Unmark(Mark::Working),
                Action::RemoveWorktree,
            ],
        ),
        (p, Event::ClosedUnmerged) if p.has_builder() => {
            block(Reason::Closed, vec![Action::Interrupt])
        }
        (p, Event::Unmarked | Event::IssueClosed) if p.has_builder() => (
            Phase::Stopped,
            vec![Action::Interrupt, Action::Unmark(Mark::Working)],
        ),
        (p, Event::Unmarked | Event::IssueClosed) if p.is_working() => {
            (Phase::Stopped, vec![Action::Unmark(Mark::Working)])
        }
        (Phase::Untracked, Event::LostTrack) => block(Reason::LostTrack, vec![]),
        (p, _) => (p, vec![]),
    }
}

/// The `index`th stage, started; Green once there are no more, and the
/// builder told so.
fn stage(limits: &Limits, index: u8, rounds: u8) -> (Phase, Vec<Action>) {
    if index < limits.stages() {
        (
            Phase::Stage { index, rounds },
            vec![Action::StartStage(index)],
        )
    } else {
        (Phase::Green { rounds }, vec![Action::TellGreen])
    }
}

/// Blocked, after `first`: the working mark goes, the blocked mark comes,
/// and a comment says why.
fn block(reason: Reason, mut first: Vec<Action>) -> (Phase, Vec<Action>) {
    first.extend([
        Action::Unmark(Mark::Working),
        Action::Mark(Mark::Blocked),
        Action::Comment(reason),
    ]);

    (Phase::Blocked(reason), first)
}

#[cfg(test)]
mod tests {
    use super::*;

    type Expected = (Phase, Vec<Action>);

    const OPEN: Phase = Phase::Open { rounds: 1 };

    fn none() -> Limits {
        Limits::new(2, []).unwrap()
    }

    fn critic() -> Limits {
        Limits::new(2, [("critic", 2), ("recording", 1)]).unwrap()
    }

    const fn stage(index: u8) -> Phase {
        Phase::Stage { index, rounds: 1 }
    }

    fn block(reason: Reason, interrupt: bool) -> Expected {
        let mut actions = if interrupt {
            vec![Action::Interrupt]
        } else {
            vec![]
        };
        actions.extend([
            Action::Unmark(Mark::Working),
            Action::Mark(Mark::Blocked),
            Action::Comment(reason),
        ]);
        (Phase::Blocked(reason), actions)
    }

    fn check(limits: &Limits, cases: Vec<(Phase, Event, Expected)>) {
        for (phase, event, expected) in cases {
            assert_eq!(
                step(phase, event, limits),
                expected,
                "{phase:?} on {event:?}"
            );
        }
    }

    #[test]
    fn starting() {
        let claim = vec![Action::Mark(Mark::Working), Action::Unmark(Mark::Build)];
        let mut reclaim = claim.clone();
        reclaim.push(Action::Unmark(Mark::Blocked));

        check(
            &none(),
            vec![
                (
                    Phase::Untracked,
                    Event::Requested,
                    (Phase::Claiming, claim.clone()),
                ),
                (Phase::Stopped, Event::Requested, (Phase::Claiming, claim)),
                (
                    Phase::Blocked(Reason::Start),
                    Event::Requested,
                    (Phase::Claiming, reclaim),
                ),
                (Phase::Done, Event::Requested, (Phase::Done, vec![])),
                (
                    Phase::Claiming,
                    Event::Claimed,
                    (Phase::Preparing, vec![Action::CreateWorktree]),
                ),
                (
                    Phase::Preparing,
                    Event::WorktreeReady,
                    (Phase::Starting, vec![Action::StartBuilder]),
                ),
                (
                    Phase::Starting,
                    Event::AgentStarted,
                    (Phase::Building, vec![Action::CommentSession]),
                ),
                (
                    Phase::Preparing,
                    Event::StartFailed,
                    block(Reason::Start, false),
                ),
                (
                    Phase::Building,
                    Event::StartFailed,
                    (Phase::Building, vec![]),
                ),
            ],
        );
    }

    #[test]
    fn following_the_pull_request() {
        let merge = vec![
            Action::CloseIssue,
            Action::Unmark(Mark::Working),
            Action::RemoveWorktree,
        ];

        check(
            &none(),
            vec![
                (
                    Phase::Building,
                    Event::PullRequestOpened,
                    (Phase::Open { rounds: 0 }, vec![]),
                ),
                (Phase::Building, Event::Pushed, (Phase::Building, vec![])),
                (Phase::Ready { rounds: 1 }, Event::Pushed, (OPEN, vec![])),
                (
                    Phase::Green { rounds: 0 },
                    Event::Pushed,
                    (Phase::Open { rounds: 0 }, vec![]),
                ),
                (
                    Phase::Open { rounds: 0 },
                    Event::RoundFailed,
                    (OPEN, vec![]),
                ),
                (OPEN, Event::RoundFailed, block(Reason::RedRounds, true)),
                (
                    OPEN,
                    Event::AllPassed,
                    (Phase::Green { rounds: 1 }, vec![Action::TellGreen]),
                ),
                (
                    Phase::Green { rounds: 1 },
                    Event::MarkedReady,
                    (Phase::Ready { rounds: 1 }, vec![]),
                ),
                (
                    Phase::Ready { rounds: 1 },
                    Event::Merged,
                    (Phase::Done, merge),
                ),
                (Phase::Building, Event::Merged, (Phase::Building, vec![])),
                (
                    Phase::Building,
                    Event::ClosedUnmerged,
                    block(Reason::Closed, true),
                ),
                (
                    Phase::Starting,
                    Event::ClosedUnmerged,
                    (Phase::Starting, vec![]),
                ),
            ],
        );
    }

    #[test]
    fn stages() {
        check(
            &critic(),
            vec![
                (
                    OPEN,
                    Event::AllPassed,
                    (stage(0), vec![Action::StartStage(0)]),
                ),
                (
                    stage(0),
                    Event::StagePassed,
                    (stage(1), vec![Action::StartStage(1)]),
                ),
                (
                    stage(1),
                    Event::StagePassed,
                    (Phase::Green { rounds: 1 }, vec![Action::TellGreen]),
                ),
                (stage(1), Event::Pushed, (OPEN, vec![])),
                (
                    stage(0),
                    Event::StageFailed { rounds: 1 },
                    (stage(0), vec![]),
                ),
                (
                    stage(0),
                    Event::StageFailed { rounds: 2 },
                    block(Reason::Stage(0), true),
                ),
                (
                    stage(1),
                    Event::StageFailed { rounds: 1 },
                    block(Reason::Stage(1), true),
                ),
                (
                    stage(2),
                    Event::StageFailed { rounds: 9 },
                    (stage(2), vec![]),
                ),
            ],
        );
    }

    #[test]
    fn stopping() {
        let stop = vec![Action::Interrupt, Action::Unmark(Mark::Working)];

        check(
            &none(),
            vec![
                (OPEN, Event::Unmarked, (Phase::Stopped, stop.clone())),
                (Phase::Building, Event::IssueClosed, (Phase::Stopped, stop)),
                (
                    Phase::Claiming,
                    Event::IssueClosed,
                    (Phase::Stopped, vec![Action::Unmark(Mark::Working)]),
                ),
                (
                    Phase::Untracked,
                    Event::Unmarked,
                    (Phase::Untracked, vec![]),
                ),
                (
                    Phase::Untracked,
                    Event::LostTrack,
                    block(Reason::LostTrack, false),
                ),
                (Phase::Stopped, Event::LostTrack, (Phase::Stopped, vec![])),
            ],
        );
    }
}
