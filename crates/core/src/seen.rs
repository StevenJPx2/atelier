//! What a poll sees of one issue, and the events it means for its phase.

use crate::{Event, Mark, Phase};

/// The marks an issue carries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Marks {
    build: bool,
    working: bool,
    blocked: bool,
}

impl Marks {
    #[must_use]
    pub const fn has(self, mark: Mark) -> bool {
        match mark {
            Mark::Build => self.build,
            Mark::Working => self.working,
            Mark::Blocked => self.blocked,
        }
    }

    #[must_use]
    pub const fn with(mut self, mark: Mark) -> Self {
        match mark {
            Mark::Build => self.build = true,
            Mark::Working => self.working = true,
            Mark::Blocked => self.blocked = true,
        }

        self
    }

    #[must_use]
    pub const fn without(mut self, mark: Mark) -> Self {
        match mark {
            Mark::Build => self.build = false,
            Mark::Working => self.working = false,
            Mark::Blocked => self.blocked = false,
        }

        self
    }
}

impl FromIterator<Mark> for Marks {
    fn from_iter<I: IntoIterator<Item = Mark>>(marks: I) -> Self {
        marks.into_iter().fold(Self::default(), Self::with)
    }
}

/// The checks on a pull request's head commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checks {
    Pending,
    Passed,
    Failed,
}

/// A pull request on an issue's branch, as its forge reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequest {
    /// `failed_rounds` counts head commits that had a failed check.
    Open {
        number: u64,
        draft: bool,
        failed_rounds: u8,
        checks: Checks,
    },
    Merged {
        number: u64,
    },
    Closed {
        number: u64,
    },
}

/// What the current stage's plugin reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageOutcome {
    Passed,
    Failed { rounds: u8 },
}

/// One issue as a poll saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    pub open: bool,
    pub marks: Marks,
    pub pull_request: Option<PullRequest>,
    pub stage: Option<StageOutcome>,
}

/// The events a poll's view means for an issue in `phase`, most urgent
/// first. Claiming has none: the worker finishes a claim from its own record.
///
/// ```
/// use atelier_core::{Checks, Event, Mark, Marks, Phase, PullRequest, Seen, events};
///
/// let seen = Seen {
///     open: true,
///     marks: Marks::default().with(Mark::Working),
///     pull_request: Some(PullRequest::Open {
///         number: 31,
///         draft: true,
///         failed_rounds: 2,
///         checks: Checks::Failed,
///     }),
///     stage: None,
/// };
///
/// assert_eq!(events(Phase::Open { rounds: 1 }, &seen), [Event::RoundFailed]);
/// ```
#[must_use]
pub fn events(phase: Phase, seen: &Seen) -> Vec<Event> {
    let mut found = Vec::new();

    if phase.has_builder() {
        match seen.pull_request {
            Some(PullRequest::Merged { .. }) if phase == Phase::Building => {
                found.push(Event::PullRequestOpened);
            }
            Some(PullRequest::Merged { .. }) => found.push(Event::Merged),
            Some(PullRequest::Closed { .. }) => found.push(Event::ClosedUnmerged),
            Some(PullRequest::Open { .. }) | None => {}
        }
    }

    if phase.is_working() && phase != Phase::Claiming {
        if !seen.open {
            found.push(Event::IssueClosed);
        } else if !seen.marks.has(Mark::Working) {
            found.push(Event::Unmarked);
        }
    }

    found.extend(progress(phase, seen));
    found
}

fn progress(phase: Phase, seen: &Seen) -> Option<Event> {
    let open = match seen.pull_request {
        Some(PullRequest::Open {
            draft,
            failed_rounds,
            checks,
            ..
        }) => Some((draft, failed_rounds, checks)),
        _ => None,
    };

    match (phase, open) {
        (Phase::Untracked | Phase::Blocked(_) | Phase::Stopped, _)
            if seen.open && seen.marks.has(Mark::Build) =>
        {
            Some(Event::Requested)
        }
        (Phase::Untracked, _) if seen.open && seen.marks.has(Mark::Working) => {
            Some(Event::LostTrack)
        }
        (Phase::Building, Some(_)) => Some(Event::PullRequestOpened),
        (Phase::Open { rounds }, Some((_, failed, checks))) => {
            if failed > rounds {
                Some(Event::RoundFailed)
            } else if checks == Checks::Passed {
                Some(Event::AllPassed)
            } else {
                None
            }
        }
        (
            Phase::Stage { rounds, .. } | Phase::Green { rounds } | Phase::Ready { rounds },
            Some((_, failed, checks)),
        ) if checks == Checks::Pending || failed > rounds => Some(Event::Pushed),
        (Phase::Stage { .. }, Some(_)) => match seen.stage {
            Some(StageOutcome::Passed) => Some(Event::StagePassed),
            Some(StageOutcome::Failed { rounds }) => Some(Event::StageFailed { rounds }),
            None => None,
        },
        (Phase::Green { .. }, Some((false, _, _))) => Some(Event::MarkedReady),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn seen(marks: Marks, pull_request: Option<PullRequest>) -> Seen {
        Seen {
            open: true,
            marks,
            pull_request,
            stage: None,
        }
    }

    const fn open(failed_rounds: u8, checks: Checks, draft: bool) -> PullRequest {
        PullRequest::Open {
            number: 1,
            draft,
            failed_rounds,
            checks,
        }
    }

    #[test]
    fn each_phase_hears_only_its_events() {
        let none = Marks::default();
        let build = none.with(Mark::Build);
        let working = none.with(Mark::Working);
        let merged = Some(PullRequest::Merged { number: 1 });

        let cases = [
            (Phase::Untracked, seen(none, None), vec![]),
            (Phase::Untracked, seen(build, None), vec![Event::Requested]),
            (
                Phase::Untracked,
                seen(working, None),
                vec![Event::LostTrack],
            ),
            (Phase::Done, seen(build, None), vec![]),
            (Phase::Claiming, seen(none, None), vec![]),
            (Phase::Preparing, seen(none, None), vec![Event::Unmarked]),
            (
                Phase::Building,
                seen(working, merged),
                vec![Event::PullRequestOpened],
            ),
            (
                Phase::Building,
                seen(working, Some(open(0, Checks::Pending, true))),
                vec![Event::PullRequestOpened],
            ),
        ];

        for (phase, seen, expected) in cases {
            assert_eq!(events(phase, &seen), expected, "{phase:?} seeing {seen:?}");
        }
    }

    #[test]
    fn a_pull_request_moves_its_builder() {
        let none = Marks::default();
        let working = none.with(Mark::Working);
        let merged = Some(PullRequest::Merged { number: 1 });
        let ready = Phase::Ready { rounds: 1 };

        let cases = [
            (ready, seen(working, merged), vec![Event::Merged]),
            (
                ready,
                seen(none, merged),
                vec![Event::Merged, Event::Unmarked],
            ),
            (
                ready,
                seen(working, Some(open(1, Checks::Passed, false))),
                vec![],
            ),
            (
                ready,
                seen(working, Some(open(2, Checks::Failed, false))),
                vec![Event::Pushed],
            ),
            (
                ready,
                seen(working, Some(open(0, Checks::Failed, false))),
                vec![],
            ),
            (
                Phase::Open { rounds: 1 },
                seen(working, Some(open(1, Checks::Failed, true))),
                vec![],
            ),
            (
                Phase::Open { rounds: 1 },
                seen(working, Some(open(1, Checks::Passed, true))),
                vec![Event::AllPassed],
            ),
            (
                Phase::Green { rounds: 0 },
                seen(working, Some(open(0, Checks::Passed, true))),
                vec![],
            ),
            (
                Phase::Green { rounds: 0 },
                seen(working, Some(open(0, Checks::Passed, false))),
                vec![Event::MarkedReady],
            ),
        ];

        for (phase, seen, expected) in cases {
            assert_eq!(events(phase, &seen), expected, "{phase:?} seeing {seen:?}");
        }
    }

    #[test]
    fn a_stage_hears_its_plugin_until_a_push() {
        let stage = Phase::Stage {
            index: 0,
            rounds: 0,
        };
        let working = Marks::default().with(Mark::Working);
        let pull = Some(open(0, Checks::Passed, true));

        let cases = [
            (None, pull, vec![]),
            (Some(StageOutcome::Passed), pull, vec![Event::StagePassed]),
            (
                Some(StageOutcome::Failed { rounds: 2 }),
                pull,
                vec![Event::StageFailed { rounds: 2 }],
            ),
            (
                Some(StageOutcome::Passed),
                Some(open(0, Checks::Pending, true)),
                vec![Event::Pushed],
            ),
        ];

        for (outcome, pull_request, expected) in cases {
            let seen = Seen {
                stage: outcome,
                ..seen(working, pull_request)
            };

            assert_eq!(events(stage, &seen), expected, "{outcome:?}");
        }
    }

    #[test]
    fn a_closed_issue_stops_work_but_not_a_merge() {
        let working = Marks::default().with(Mark::Working);
        let closed = Seen {
            open: false,
            ..seen(working, Some(PullRequest::Merged { number: 1 }))
        };

        assert_eq!(
            events(
                Phase::Building,
                &Seen {
                    pull_request: None,
                    ..closed
                }
            ),
            [Event::IssueClosed]
        );
        assert_eq!(
            events(Phase::Open { rounds: 0 }, &closed),
            [Event::Merged, Event::IssueClosed]
        );
        assert_eq!(events(Phase::Stopped, &closed), []);
    }
}
