//! Kani harnesses for the issue machine.

use super::limits::MAX_STAGES;
use super::{Action, Event, Limits, Mark, Phase, step};

fn any_limits(count: u8) -> Limits {
    let red_rounds: u8 = kani::any();
    kani::assume((1..=5).contains(&red_rounds));

    let limits: [u8; MAX_STAGES] = kani::any();

    for (index, limit) in limits.iter().enumerate() {
        if index < usize::from(count) {
            kani::assume((1..=2).contains(limit));
        }
    }

    Limits::unnamed(red_rounds, limits, count)
}

fn reachable(phase: Phase, limits: &Limits) -> bool {
    let rounds_fit = phase.rounds() <= limits.red_rounds();

    let index_fits = match phase {
        Phase::Stage { index, .. } => index < limits.stages(),
        _ => true,
    };

    rounds_fit && index_fits
}

fn fair(event: Event) -> bool {
    match event {
        Event::StageFailed { rounds } => rounds <= 2,
        _ => true,
    }
}

/// For every phase an issue can be in, every event, and all limits of
/// `count` stages, `step` keeps the machine's promises.
fn step_keeps_its_promises(count: u8) {
    let limits = any_limits(count);
    let phase: Phase = kani::any();
    let event: Event = kani::any();
    kani::assume(reachable(phase, &limits) && fair(event));

    let (next, actions) = step(phase, event, &limits);
    let has = |action: Action| actions.contains(&action);
    let stages = limits.stages();

    assert_eq!(
        has(Action::StartBuilder),
        phase == Phase::Preparing && event == Event::WorktreeReady
    );

    if next == Phase::Starting && phase != Phase::Starting {
        assert!(phase == Phase::Preparing && event == Event::WorktreeReady);
    }

    if next == Phase::Claiming && phase != Phase::Claiming {
        assert_eq!(event, Event::Requested);
    }

    if phase.is_working() && !next.is_working() {
        assert!(has(Action::Unmark(Mark::Working)));
    }

    if phase.has_builder() && !next.is_working() && next != Phase::Done {
        assert!(has(Action::Interrupt));
    }

    if matches!(next, Phase::Green { .. }) && !matches!(phase, Phase::Green { .. }) {
        let last = matches!(phase, Phase::Stage { index, .. } if index + 1 == stages)
            && event == Event::StagePassed;
        let straight =
            matches!(phase, Phase::Open { .. }) && event == Event::AllPassed && stages == 0;

        assert!(last || straight);
    }

    let entered_green =
        matches!(next, Phase::Green { .. }) && !matches!(phase, Phase::Green { .. });
    assert_eq!(has(Action::TellGreen), entered_green);

    if event == Event::Merged && phase.has_pull_request() {
        assert!(has(Action::CloseIssue));
    }

    assert!(reachable(next, &limits));

    if next == phase {
        assert!(actions.is_empty());
    }
}

#[kani::proof]
#[kani::unwind(9)]
fn with_no_stages() {
    step_keeps_its_promises(0);
}

#[kani::proof]
#[kani::unwind(9)]
fn with_one_stage() {
    step_keeps_its_promises(1);
}

#[kani::proof]
#[kani::unwind(9)]
fn with_two_stages() {
    step_keeps_its_promises(2);
}

#[kani::proof]
#[kani::unwind(9)]
fn with_three_stages() {
    step_keeps_its_promises(3);
}
