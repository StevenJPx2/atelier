//! Kani harnesses for what core adds over rungs, which proves its own
//! promises: for every answer rungs can give, the run's phase and actions
//! follow it; and for every run and event rungs plays no part in, every
//! move keeps the marks and the resident steps right.

use rungs::{MAX_STEPS, Outcome, Progress, State, Status};

use super::step::{conclude, outside};
use super::{Action, Event, Mark, Phase, Reason, Run, step};

/// Any progress over `count` steps: each status, with an outcome of the
/// first two or `failed`, and up to two failed rounds; the rest pending.
fn any_progress(count: usize) -> Progress {
    let mut progress = Progress::default();
    let positions = progress.status.iter_mut().zip(progress.failed.iter_mut());

    for (status, failed) in positions.take(count) {
        let kind: u8 = kani::any();
        let rounds: u8 = kani::any();
        kani::assume(kind <= 5 && rounds <= 2);

        *status = match kind {
            0 => Status::Pending,
            1 => Status::Running,
            2 => Status::Skipped,
            3 => Status::Settled(Outcome(0)),
            4 => Status::Settled(Outcome(1)),
            _ => Status::Settled(Outcome::FAILED),
        };
        *failed = rounds;
    }

    progress
}

/// Any state rungs reports: running, or over with any unhandled step.
fn any_state() -> State {
    let over: bool = kani::any();
    let unhandled: Option<usize> = kani::any();
    kani::assume(unhandled.is_none_or(|step| step < MAX_STEPS));

    if over {
        State::Over { unhandled }
    } else {
        State::Running
    }
}

/// What a list of actions does, read in one pass.
#[derive(Default)]
struct Does {
    starts: [bool; MAX_STEPS],
    interrupt: bool,
    report: bool,
    close: bool,
    working: bool,
    unworking: bool,
    unbuild: bool,
    blocked: bool,
    remove: bool,
}

fn does(actions: &[Action]) -> Does {
    let mut does = Does::default();

    for action in actions {
        match *action {
            Action::Start(step) => {
                if let Some(started) = does.starts.get_mut(usize::from(step)) {
                    *started = true;
                }
            }
            Action::Interrupt => does.interrupt = true,
            Action::Report => does.report = true,
            Action::CloseIssue => does.close = true,
            Action::Mark(Mark::Working) => does.working = true,
            Action::Unmark(Mark::Working) => does.unworking = true,
            Action::Unmark(Mark::Build) => does.unbuild = true,
            Action::Mark(Mark::Blocked) => does.blocked = true,
            Action::RemoveWorktree => does.remove = true,
            _ => {}
        }
    }

    does
}

impl Does {
    /// Every way out of a running run interrupts its resident steps,
    /// removes the working mark and removes the worktree.
    const fn leaves(&self) -> bool {
        self.interrupt && self.unworking && self.remove
    }
}

/// After rungs moved a run: a step starts exactly when rungs started it,
/// the run goes on exactly while rungs says it runs, Done comes only when
/// no step runs and none can start, and a final failure no step waits for
/// blocks the run at that step.
#[kani::proof]
#[kani::unwind(12)]
fn a_run_follows_what_rungs_answers() {
    let started: [bool; MAX_STEPS] = kani::any();
    let state = any_state();

    let (run, actions) = conclude(started, state, Progress::default(), Vec::new());
    let does = does(&actions);

    assert_eq!(does.starts, started);

    match state {
        State::Running => {
            assert_eq!(run.phase, Phase::Running);
            assert!(!does.unworking && !does.interrupt);
        }
        State::Over { unhandled: None } => {
            assert_eq!(run.phase, Phase::Done);
            assert!(does.leaves() && does.report && does.close);
        }
        State::Over {
            unhandled: Some(index),
        } => {
            let step = u8::try_from(index).unwrap_or(u8::MAX);

            assert_eq!(run.phase, Phase::Blocked(Reason::Failed(step)));
            assert!(does.leaves() && does.blocked);
        }
    }
}

/// For every run and every event rungs plays no part in: an event that
/// does not apply changes nothing, a claim comes only from a request and
/// starts fresh, and every way out of a working run removes the working
/// mark, and of a running one interrupts its resident steps and removes
/// the worktree. Only a claim's confirmation and a running step's events
/// go to rungs.
#[kani::proof]
#[kani::unwind(12)]
fn a_move_outside_rungs_keeps_the_marks_and_the_resident_steps_right() {
    let run = Run {
        phase: kani::any(),
        progress: any_progress(MAX_STEPS),
    };
    let event: Event = kani::any();

    let Some((next, actions)) = outside(run, event) else {
        let claimed = run.phase == Phase::Claiming && event == Event::Claimed;
        let moves_a_step = matches!(
            event,
            Event::Settled { .. }
                | Event::Failed { .. }
                | Event::StartFailed { .. }
                | Event::Pushed { .. }
        );

        assert!(claimed || (run.phase == Phase::Running && moves_a_step));
        return;
    };
    let does = does(&actions);

    if next == run {
        assert!(actions.is_empty());
    }

    if next.phase == Phase::Claiming && run.phase != Phase::Claiming {
        assert_eq!(event, Event::Requested);
        assert_eq!(next.progress, Progress::default());
        assert!(does.working && does.unbuild);
    }

    if run.phase.is_working() && !next.phase.is_working() {
        assert!(does.unworking);
    }

    if run.phase == Phase::Running && next.phase != Phase::Running {
        assert!(does.leaves());
    }

    assert!(next.phase != Phase::Done || run.phase == Phase::Done);
    assert!(next.phase != Phase::Running || run.phase == Phase::Running);
}
