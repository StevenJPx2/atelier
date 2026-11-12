//! A poll through `observe` and `act`, against an in-memory world.

use crate::world::{self, World};
use atelier_core::{
    Action, Checks, Failure, Filter, IssueKey, Mark, Marks, Phase, PullRequest, Reason, Record,
    State, act, observe,
};

fn key() -> IssueKey {
    IssueKey {
        tracker: "mem".into(),
        id: "12".into(),
    }
}

fn phase(state: &State) -> Phase {
    state
        .issues
        .get(&key())
        .map_or(Phase::Untracked, |record| record.phase)
}

fn building() -> (World, State) {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Build]);
    let mut state = State::default();
    world.poll(&world::project(&[]), &mut state);
    (world, state)
}

const fn open_pull(failed_rounds: u8, checks: Checks, draft: bool) -> PullRequest {
    PullRequest::Open {
        number: 31,
        draft,
        failed_rounds,
        checks,
    }
}

#[test]
fn a_requested_issue_starts_one_builder_across_many_polls() {
    let (world, mut state) = building();
    let project = world::project(&[]);

    for _ in 0..5 {
        world.poll(&project, &mut state);
    }

    let record = &state.issues[&key()];
    let ticket = world.ticket("12");

    assert_eq!(record.phase, Phase::Building);
    assert_eq!(record.branch, "issue-12-fix-the-dead-link");
    assert_eq!(world.sessions.borrow().len(), 1);
    assert_eq!(ticket.marks, Marks::default().with(Mark::Working));
    assert_eq!(ticket.comments.len(), 1);
    assert!(ticket.comments[0].contains("ses_0"));
    assert!(ticket.comments[0].contains("issue-12-fix-the-dead-link"));

    let brief = &world.briefs.borrow()[0];
    assert_eq!(brief.title, "#12 Fix the dead link");
    assert!(brief.text().contains("Read #12"));
    assert!(brief.text().contains("into v2"));
    assert!(brief.denied.contains(&"merge *".to_owned()));
}

/// A stop after any save resumes from that save: the issue ends where an
/// uninterrupted run ends, with every mark and the worktree as they were.
#[test]
fn a_stop_after_any_save_resumes_to_the_same_end() {
    let (clean, clean_state) = building();

    for crash_at in 1..=12 {
        let world = World::with_issue("12", "Fix the dead link", &[Mark::Build]);
        let project = world::project(&[]);
        let plugins = world.plugins();
        let mut saved = State::default();
        let mut working = saved.clone();
        let mut count = 0;

        let observation = observe(&project, &plugins, &saved).unwrap();
        let mut save = |state: &State| {
            count += 1;
            if count == crash_at {
                return Err(());
            }
            saved = state.clone();
            Ok(())
        };
        let _ = act(&observation, &mut working, &project, &plugins, &mut save);

        let mut state = saved;
        for _ in 0..3 {
            world.poll(&project, &mut state);
        }

        assert_eq!(phase(&state), Phase::Building, "stop at save {crash_at}");
        assert_eq!(
            world.ticket("12").marks,
            clean.ticket("12").marks,
            "stop at save {crash_at}"
        );
        assert_eq!(
            *world.worktrees.borrow(),
            *clean.worktrees.borrow(),
            "stop at save {crash_at}"
        );
        assert!(
            world.sessions.borrow().len() <= 2,
            "stop at save {crash_at}"
        );
        assert_eq!(
            state.issues[&key()].worktree,
            clean_state.issues[&key()].worktree
        );
    }
}

#[test]
fn a_transient_failure_retries_and_keeps_the_listing_unconfirmed() {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Build]);
    let project = world::project(&[]);
    let mut state = State::default();
    *world.fail.borrow_mut() = Some((3, Failure::Transient));

    let report = world.poll(&project, &mut state);

    assert_eq!(report.failures.len(), 1);
    assert_eq!(phase(&state), Phase::Preparing);
    assert!(state.etags.is_empty());

    let report = world.poll(&project, &mut state);

    assert_eq!(report.failures, []);
    assert_eq!(phase(&state), Phase::Building);
    assert_eq!(state.etags.len(), 2);
}

#[test]
fn a_permanent_start_failure_blocks_the_issue() {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Build]);
    let project = world::project(&[]);
    let mut state = State::default();
    *world.fail.borrow_mut() = Some((3, Failure::Permanent));

    world.poll(&project, &mut state);

    let ticket = world.ticket("12");
    assert_eq!(phase(&state), Phase::Blocked(Reason::Start));
    assert_eq!(ticket.marks, Marks::default().with(Mark::Blocked));
    assert!(ticket.comments[0].contains("could not start"));
    assert!(world.sessions.borrow().is_empty());
}

#[test]
fn a_blocked_issue_requested_again_continues_on_its_branch() {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Build]);
    let project = world::project(&[]);
    let mut state = State::default();
    *world.fail.borrow_mut() = Some((3, Failure::Permanent));
    world.poll(&project, &mut state);
    *world.fail.borrow_mut() = None;

    world.edit("12", |ticket| ticket.marks = ticket.marks.with(Mark::Build));
    world.poll(&project, &mut state);

    assert_eq!(phase(&state), Phase::Building);
    assert_eq!(
        world.ticket("12").marks,
        Marks::default().with(Mark::Working)
    );
    assert_eq!(state.issues[&key()].branch, "issue-12-fix-the-dead-link");
}

#[test]
fn removing_the_working_mark_stops_the_builder() {
    let (world, mut state) = building();

    world.edit("12", |ticket| {
        ticket.marks = ticket.marks.without(Mark::Working);
    });
    world.poll(&world::project(&[]), &mut state);

    assert_eq!(phase(&state), Phase::Stopped);
    assert_eq!(*world.interrupts.borrow(), ["ses_0"]);
}

#[test]
fn closing_the_issue_stops_the_builder_and_drops_the_working_mark() {
    let (world, mut state) = building();

    world.edit("12", |ticket| ticket.open = false);
    world.poll(&world::project(&[]), &mut state);

    assert_eq!(phase(&state), Phase::Stopped);
    assert_eq!(*world.interrupts.borrow(), ["ses_0"]);
    assert!(!world.ticket("12").marks.has(Mark::Working));
}

#[test]
fn failed_rounds_up_to_the_limit_block_the_issue() {
    let (world, mut state) = building();
    let project = world::project(&[]);
    let branch = state.issues[&key()].branch.clone();

    for (failed, expected) in [
        (0, Phase::Open { rounds: 0 }),
        (2, Phase::Open { rounds: 2 }),
        (3, Phase::Blocked(Reason::RedRounds)),
    ] {
        world
            .pulls
            .borrow_mut()
            .insert(branch.clone(), open_pull(failed, Checks::Failed, true));
        world.poll(&project, &mut state);
        assert_eq!(phase(&state), expected, "{failed} failed rounds");
    }

    let ticket = world.ticket("12");
    assert_eq!(ticket.marks, Marks::default().with(Mark::Blocked));
    assert!(ticket.comments.last().unwrap().contains("failed 3 rounds"));
    assert_eq!(*world.interrupts.borrow(), ["ses_0"]);
}

#[test]
fn passing_checks_and_a_ready_pull_request_reach_ready_then_merge_closes_it() {
    let (world, mut state) = building();
    let project = world::project(&[]);
    let branch = state.issues[&key()].branch.clone();
    let set = |pull| world.pulls.borrow_mut().insert(branch.clone(), pull);

    assert_eq!(state.issues[&key()].pull_request, None);

    set(open_pull(1, Checks::Passed, true));
    world.poll(&project, &mut state);
    assert_eq!(phase(&state), Phase::Green { rounds: 1 });
    assert_eq!(state.issues[&key()].pull_request, Some(31));

    set(open_pull(1, Checks::Passed, false));
    world.poll(&project, &mut state);
    assert_eq!(phase(&state), Phase::Ready { rounds: 1 });

    set(open_pull(1, Checks::Pending, false));
    world.poll(&project, &mut state);
    assert_eq!(phase(&state), Phase::Open { rounds: 1 });

    set(PullRequest::Merged { number: 32 });
    world.poll(&project, &mut state);
    assert_eq!(state.issues[&key()].pull_request, Some(32));

    let ticket = world.ticket("12");
    assert_eq!(phase(&state), Phase::Done);
    assert!(!ticket.open);
    assert!(!ticket.marks.has(Mark::Working));
    assert!(world.worktrees.borrow().is_empty());
    assert!(world.interrupts.borrow().is_empty());
}

#[test]
fn a_pull_request_closed_unmerged_blocks_the_issue() {
    let (world, mut state) = building();
    let branch = state.issues[&key()].branch.clone();

    world
        .pulls
        .borrow_mut()
        .insert(branch, PullRequest::Closed { number: 31 });
    world.poll(&world::project(&[]), &mut state);

    assert_eq!(phase(&state), Phase::Blocked(Reason::Closed));
    assert_eq!(*world.interrupts.borrow(), ["ses_0"]);
}

#[test]
fn a_stage_with_no_plugin_holds_the_issue_and_reports_it() {
    let (world, mut state) = building();
    let project = world::project(&[("critic", 2)]);
    let branch = state.issues[&key()].branch.clone();

    world
        .pulls
        .borrow_mut()
        .insert(branch, open_pull(0, Checks::Passed, true));
    let report = world.poll(&project, &mut state);

    assert_eq!(
        phase(&state),
        Phase::Stage {
            index: 0,
            rounds: 0
        }
    );
    assert!(report.failures[0].1.message.contains("critic"));
}

#[test]
fn an_issue_marked_working_that_atelier_never_took_is_blocked() {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Working]);
    let mut state = State::default();

    world.poll(&world::project(&[]), &mut state);

    let ticket = world.ticket("12");
    assert_eq!(phase(&state), Phase::Blocked(Reason::LostTrack));
    assert_eq!(ticket.marks, Marks::default().with(Mark::Blocked));
    assert!(ticket.comments[0].contains("no record"));
}

#[test]
fn an_unmarked_issue_is_left_alone() {
    let world = World::with_issue("12", "Fix the dead link", &[]);
    let mut state = State::default();

    let report = world.poll(&world::project(&[]), &mut state);

    assert_eq!(report.steps, []);
    assert!(state.issues.is_empty());
    assert_eq!(*world.changes.borrow(), 0);
}

#[test]
fn a_finished_issue_is_not_read_again() {
    let (world, mut state) = building();
    let project = world::project(&[]);
    let branch = state.issues[&key()].branch.clone();
    world
        .pulls
        .borrow_mut()
        .insert(branch, PullRequest::Merged { number: 31 });
    world.poll(&project, &mut state);
    let reads = *world.reads.borrow();

    world.poll(&project, &mut state);

    assert_eq!(phase(&state), Phase::Done);
    assert_eq!(*world.reads.borrow(), reads);
}

#[test]
fn a_builder_without_a_worktree_is_blocked_not_started() {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Working]);
    let mut state = State::default();
    let mut record = Record::new(&key(), "Fix the dead link");
    record.phase = Phase::Starting;
    record.pending = vec![Action::StartBuilder];
    state.issues.insert(key(), record);

    world.poll(&world::project(&[]), &mut state);

    assert_eq!(phase(&state), Phase::Blocked(Reason::Start));
    assert_eq!(world.sessions.borrow().len(), 0);
}

#[test]
fn the_filter_narrows_requests_but_not_lost_track() {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Build]);
    world.edit("12", |ticket| ticket.assignee = Some("someone-else".into()));
    let copy = world.ticket("12");
    world.tickets.borrow_mut().insert("13".into(), copy);
    world.edit("13", |ticket| {
        ticket.marks = Marks::default().with(Mark::Working);
    });
    *world.filter.borrow_mut() = Filter {
        assignee: Some("me".into()),
        ..Filter::default()
    };
    let mut state = State::default();

    world.poll(&world::project(&[]), &mut state);

    let lost = IssueKey {
        tracker: "mem".into(),
        id: "13".into(),
    };
    assert!(!state.issues.contains_key(&key()));
    assert_eq!(state.issues[&lost].phase, Phase::Blocked(Reason::LostTrack));
}
