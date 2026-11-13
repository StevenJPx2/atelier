//! A poll through `observe` and `act`, against an in-memory world.

use atelier_core::rungs::{Outcome, Progress, Status};
use atelier_core::{
    Action, Failure, Filter, IssueKey, Mark, Marks, Observed, Phase, PullRequest, Reason, Record,
    Rollup, State, StepRecord, act, observe,
};

use crate::world::{World, list, step, table};

fn key() -> IssueKey {
    IssueKey {
        tracker: "mem".into(),
        id: "12".into(),
    }
}

fn phase(state: &State) -> Phase {
    state
        .runs
        .get(&key())
        .map_or(Phase::Untracked, |record| record.phase)
}

/// The status of the step called `id`.
fn status(state: &State, id: &str) -> Option<Status> {
    let record = state.runs.get(&key())?;
    let index = record.steps.iter().position(|step| step.id == id)?;

    record.progress.status.get(index).copied()
}

fn building() -> (World, State) {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Build]);
    let mut state = State::default();
    world.poll(&world.project(), &mut state);
    (world, state)
}

const fn open_pull(failed_rounds: u8, checks: Rollup, draft: bool) -> PullRequest {
    PullRequest::Open {
        number: 31,
        draft,
        failed_rounds,
        checks,
    }
}

/// The builder was interrupted once, then told `why` and to act on nothing
/// later.
fn stopped(world: &World, why: &str) {
    let told = world.told.borrow();

    assert_eq!(*world.interrupts.borrow(), ["ses_0"]);
    assert!(
        matches!(
            told.last(),
            Some((session, text)) if session == "ses_0"
                && text.contains(why)
                && text.contains("Take no further action")
        ),
        "{told:?}"
    );
}

#[test]
fn a_requested_issue_starts_one_builder_across_many_polls() {
    let (world, mut state) = building();
    let project = world.project();

    for _ in 0..5 {
        world.poll(&project, &mut state);
    }

    let record = &state.runs[&key()];
    let ticket = world.ticket("12");

    assert_eq!(record.phase, Phase::Running);
    assert_eq!(record.pipeline, "build");
    assert_eq!(
        status(&state, "worktree"),
        Some(Status::Settled(Outcome(0)))
    );
    assert_eq!(status(&state, "builder"), Some(Status::Running));
    assert_eq!(record.branch, "issue-12-fix-the-dead-link");
    assert_eq!(*world.started.borrow(), ["worktree", "builder"]);
    assert_eq!(world.sessions.borrow().len(), 1);
    assert_eq!(ticket.marks, Marks::default().with(Mark::Working));
    assert_eq!(ticket.comments.len(), 1);
    assert!(ticket.comments[0].contains("`builder`"));
    assert!(ticket.comments[0].contains("ses_0"));
    assert!(ticket.comments[0].contains("issue-12-fix-the-dead-link"));

    let brief = &world.briefs.borrow()[0];
    assert_eq!(brief.title, "#12 Fix the dead link");
    assert!(brief.text().contains("Read #12"));
    assert!(brief.text().contains("into v2"));
    assert!(brief.denied.contains(&"merge *".to_owned()));
}

/// A stop after any save resumes from that save: the run ends where an
/// uninterrupted run ends, with every mark and the worktree as they were.
#[test]
fn a_stop_after_any_save_resumes_to_the_same_end() {
    let (clean, clean_state) = building();

    for crash_at in 1..=12 {
        let world = World::with_issue("12", "Fix the dead link", &[Mark::Build]);
        let project = world.project();
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

        assert_eq!(phase(&state), Phase::Running, "stop at save {crash_at}");
        assert_eq!(
            status(&state, "builder"),
            Some(Status::Running),
            "stop at save {crash_at}"
        );
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
            state.runs[&key()].worktree,
            clean_state.runs[&key()].worktree
        );
    }
}

#[test]
fn a_settled_step_without_restart_is_not_observed_again() {
    let (world, mut state) = building();
    world.observed.borrow_mut().clear();

    world.poll(&world.project(), &mut state);

    assert_eq!(*world.observed.borrow(), ["builder"]);
    assert_eq!(
        status(&state, "worktree"),
        Some(Status::Settled(Outcome(0)))
    );
}

#[test]
fn a_stale_listing_of_a_closed_or_unmarked_item_starts_no_run() {
    use atelier_core::{Issue, Observation, Seen, Watched};
    use std::collections::BTreeMap;

    for (open, marks) in [
        (false, Marks::default().with(Mark::Working)),
        (true, Marks::default()),
    ] {
        let world = World::with_issue("12", "Fix the dead link", &[]);
        let issue = Issue {
            key: key(),
            title: "Fix the dead link".into(),
            open,
            marks,
        };
        let watched = Watched {
            issue,
            seen: Seen {
                open,
                marks,
                pull_request: None,
            },
        };
        let observation = Observation {
            issues: BTreeMap::from([(key(), watched)]),
            ..Observation::default()
        };
        let mut state = State::default();

        let report = act(
            &observation,
            &mut state,
            &world.project(),
            &world.plugins(),
            &mut |_| Ok::<(), ()>(()),
        )
        .unwrap();

        assert_eq!(report.moves, []);
        assert!(state.runs.is_empty());
    }
}

#[expect(
    clippy::unwrap_used,
    reason = "test fixture: the in-memory world must answer and save"
)]
fn requested(
    world: &World,
    state: &mut State,
    request: atelier_core::Request,
) -> atelier_core::Report {
    let project = world.project();
    let plugins = world.plugins();
    let mut observation = observe(&project, &plugins, state).unwrap();
    observation.requests.insert(key(), vec![request]);

    act(&observation, state, &project, &plugins, &mut |_| {
        Ok::<(), ()>(())
    })
    .unwrap()
}

#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test fixture: building holds its builder session"
)]
fn finish(state: &State, name: &str) -> atelier_core::Request {
    let record = &state.runs[&key()];
    atelier_core::Request {
        generation: record.generation,
        command: atelier_core::Command::Finish {
            step: "builder".into(),
            outcome: atelier_core::Outcome {
                name: name.into(),
                why: "nothing needs changing".into(),
            },
            session: record.steps[1].session.as_ref().unwrap().id.clone(),
            failed: record.progress.failed[1],
        },
    }
}

#[test]
fn a_finish_request_settles_its_step_and_is_harmless_when_replayed() {
    let (world, mut state) = building();
    let request = finish(&state, "nothing");

    requested(&world, &mut state, request.clone());
    let done = state.clone();
    requested(&world, &mut state, request);

    assert_eq!(phase(&state), Phase::Done);
    assert_eq!(state, done);
    assert_eq!(status(&state, "builder"), Some(Status::Settled(Outcome(1))));
    assert!(
        world
            .ticket("12")
            .comments
            .last()
            .unwrap()
            .contains("nothing needs changing")
    );
}

#[test]
fn stale_or_invalid_finish_requests_leave_the_run_alone() {
    let (world, mut state) = building();
    let good = finish(&state, "nothing");
    let mut old_pass = good.clone();
    old_pass.generation = old_pass.generation.saturating_sub(1);
    let mut old_session = good.clone();
    if let atelier_core::Command::Finish { session, .. } = &mut old_session.command {
        *session = "replaced-session".into();
    }
    let mut old_round = good;
    if let atelier_core::Command::Finish { failed, .. } = &mut old_round.command {
        *failed = 1;
    }
    let unknown = finish(&state, "unknown");

    for request in [old_pass, old_session, old_round, unknown] {
        let before = state.clone();

        let report = requested(&world, &mut state, request);

        assert_eq!(state, before);
        assert_eq!(report.moves, []);
    }
}

#[test]
fn a_stop_request_preempts_pending_starts_and_removes_the_request_mark() {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Build]);
    let mut state = State::default();
    let mut record = Record::new(&key(), "Fix the dead link", "build");
    record.begin(&world.project().pipelines[0]);
    record.phase = Phase::Claiming;
    record.pending = vec![Action::Mark(Mark::Working), Action::Unmark(Mark::Build)];
    let request = atelier_core::Request {
        generation: record.generation,
        command: atelier_core::Command::Stop,
    };
    state.runs.insert(key(), record);

    requested(&world, &mut state, request);

    assert_eq!(phase(&state), Phase::Stopped);
    assert_eq!(world.ticket("12").marks, Marks::default());
    assert_eq!(*world.started.borrow(), Vec::<String>::new());
}

#[test]
fn a_stop_request_takes_priority_over_a_queued_finish() {
    let (world, mut state) = building();
    let finish = finish(&state, "nothing");
    let stop = atelier_core::Request {
        generation: state.runs[&key()].generation,
        command: atelier_core::Command::Stop,
    };
    let project = world.project();
    let plugins = world.plugins();
    let mut observation = observe(&project, &plugins, &state).unwrap();
    observation.requests.insert(key(), vec![finish, stop]);

    act(&observation, &mut state, &project, &plugins, &mut |_| {
        Ok::<(), ()>(())
    })
    .unwrap();

    assert_eq!(phase(&state), Phase::Stopped);
    assert!(world.ticket("12").open);
}

#[test]
fn a_transient_failure_retries_and_keeps_the_listing_unconfirmed() {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Build]);
    let project = world.project();
    let mut state = State::default();
    *world.fail.borrow_mut() = Some((3, Failure::Transient));

    let report = world.poll(&project, &mut state);

    assert_eq!(report.failures.len(), 1);
    assert_eq!(phase(&state), Phase::Running);
    assert_eq!(status(&state, "worktree"), Some(Status::Running));
    assert!(state.etags.is_empty());

    let report = world.poll(&project, &mut state);

    assert_eq!(report.failures, []);
    assert_eq!(status(&state, "builder"), Some(Status::Running));
    assert_eq!(state.etags.len(), 2);
}

#[test]
fn a_permanent_start_failure_blocks_the_run() {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Build]);
    let project = world.project();
    let mut state = State::default();
    *world.fail.borrow_mut() = Some((3, Failure::Permanent));

    world.poll(&project, &mut state);

    let ticket = world.ticket("12");
    assert_eq!(phase(&state), Phase::Blocked(Reason::Start(0)));
    assert_eq!(ticket.marks, Marks::default().with(Mark::Blocked));
    assert!(ticket.comments[0].contains("could not start `worktree`"));
    assert!(ticket.comments[0].contains("call 3 fails"));
    assert!(world.sessions.borrow().is_empty());
}

#[test]
fn a_claim_that_fails_for_good_blocks_the_run() {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Build]);
    let mut state = State::default();
    *world.fail.borrow_mut() = Some((1, Failure::Permanent));

    world.poll(&world.project(), &mut state);

    assert_eq!(phase(&state), Phase::Blocked(Reason::Claim));
    assert!(world.ticket("12").comments[0].contains("could not claim"));
}

#[test]
fn a_fresh_pass_keeps_each_steps_own_previous_session() {
    let (world, state) = building();
    let project = world.project();
    let mut record = state.runs[&key()].clone();
    let previous = record.steps[1].session.clone();

    record.begin(&project.pipelines[0]);

    assert!(previous.is_some());
    assert_eq!(record.steps[1].session, previous);
    assert_eq!(record.steps[0].session, None);
    assert_eq!(record.steps[2].session, None);
}

#[test]
fn a_blocked_run_requested_again_continues_on_its_branch() {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Build]);
    let project = world.project();
    let mut state = State::default();
    *world.fail.borrow_mut() = Some((3, Failure::Permanent));
    world.poll(&project, &mut state);
    *world.fail.borrow_mut() = None;

    world.edit("12", |ticket| ticket.marks = ticket.marks.with(Mark::Build));
    world.poll(&project, &mut state);

    assert_eq!(phase(&state), Phase::Running);
    assert_eq!(status(&state, "builder"), Some(Status::Running));
    assert_eq!(
        world.ticket("12").marks,
        Marks::default().with(Mark::Working)
    );
    assert_eq!(state.runs[&key()].branch, "issue-12-fix-the-dead-link");
}

#[test]
fn removing_the_working_mark_stops_the_run_and_removes_its_worktree() {
    let (world, mut state) = building();

    world.edit("12", |ticket| {
        ticket.marks = ticket.marks.without(Mark::Working);
    });
    world.poll(&world.project(), &mut state);

    assert_eq!(phase(&state), Phase::Stopped);
    stopped(&world, "lost its working mark or was closed");
    assert!(world.worktrees.borrow().is_empty());
}

#[test]
fn closing_the_issue_stops_the_builder_and_drops_the_working_mark() {
    let (world, mut state) = building();

    world.edit("12", |ticket| ticket.open = false);
    world.poll(&world.project(), &mut state);

    assert_eq!(phase(&state), Phase::Stopped);
    stopped(&world, "lost its working mark or was closed");
    assert!(!world.ticket("12").marks.has(Mark::Working));
}

#[test]
fn failed_rounds_up_to_the_limit_block_the_run() {
    let (world, mut state) = building();
    let project = world.project();
    let branch = state.runs[&key()].branch.clone();

    for (failed, rounds, expected) in [
        (0, 0, Phase::Running),
        (2, 2, Phase::Running),
        (3, 3, Phase::Blocked(Reason::Failed(2))),
    ] {
        world
            .pulls
            .borrow_mut()
            .insert(branch.clone(), open_pull(failed, Rollup::Failed, true));
        world.poll(&project, &mut state);
        assert_eq!(phase(&state), expected, "{failed} failed rounds");
        assert_eq!(state.runs[&key()].progress.failed[2], rounds);
    }

    let ticket = world.ticket("12");
    assert_eq!(ticket.marks, Marks::default().with(Mark::Blocked));
    assert!(
        ticket
            .comments
            .last()
            .unwrap()
            .contains("`checks` failed: CI failed on 3 commits")
    );
    stopped(&world, "CI failed on 3 commits");
    assert!(world.worktrees.borrow().is_empty());
}

#[test]
fn passing_checks_and_a_ready_pull_request_reach_the_merge_which_closes_it() {
    let (world, mut state) = building();
    let project = world.project();
    let branch = state.runs[&key()].branch.clone();
    let set = |pull| world.pulls.borrow_mut().insert(branch.clone(), pull);

    assert_eq!(state.runs[&key()].pull_request, None);

    set(open_pull(1, Rollup::Passed, true));
    world.poll(&project, &mut state);
    assert_eq!(status(&state, "builder"), Some(Status::Settled(Outcome(0))));
    assert_eq!(status(&state, "checks"), Some(Status::Settled(Outcome(0))));
    assert_eq!(status(&state, "ready"), Some(Status::Running));
    assert_eq!(state.runs[&key()].progress.failed[2], 1);
    assert_eq!(state.runs[&key()].pull_request, Some(31));
    let told = world.told.borrow().clone();
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].0, "ses_0");
    assert!(told[0].1.contains("Every check passed"), "{told:?}");
    assert!(told[0].1.contains("Mark it ready"), "{told:?}");

    world.pulls.borrow_mut().remove(&branch);
    world.poll(&project, &mut state);
    assert_eq!(state.runs[&key()].pull_request, Some(31));
    assert_eq!(status(&state, "ready"), Some(Status::Running));

    set(open_pull(1, Rollup::Passed, false));
    world.poll(&project, &mut state);
    assert_eq!(status(&state, "merge"), Some(Status::Running));

    set(open_pull(1, Rollup::Pending, false));
    world.poll(&project, &mut state);
    assert_eq!(status(&state, "checks"), Some(Status::Running));
    assert_eq!(status(&state, "ready"), Some(Status::Pending));
    assert_eq!(status(&state, "merge"), Some(Status::Pending));
    assert_eq!(
        world.told.borrow().len(),
        1,
        "told again without a new green"
    );

    set(open_pull(1, Rollup::Passed, true));
    world.poll(&project, &mut state);
    assert_eq!(status(&state, "ready"), Some(Status::Running));
    assert_eq!(world.told.borrow().len(), 2, "a new green is told");

    set(PullRequest::Merged { number: 32 });
    world.poll(&project, &mut state);
    assert_eq!(state.runs[&key()].pull_request, Some(32));

    let ticket = world.ticket("12");
    let report = &ticket.comments[ticket.comments.len() - 1];
    assert_eq!(phase(&state), Phase::Done);
    assert!(!ticket.open);
    assert!(!ticket.marks.has(Mark::Working));
    assert!(world.worktrees.borrow().is_empty());
    stopped(&world, "atelier finished this run of build");
    assert!(report.starts_with("atelier finished `build`:"), "{report}");
    assert!(
        report.contains("- `checks`: passed (every check passed)")
            && report.contains("- `ready`: passed (#32 merged)"),
        "{report}"
    );
    assert!(
        report.contains("- `merge`: merged (#32 merged)"),
        "{report}"
    );
}

#[test]
fn a_pull_request_closed_unmerged_blocks_the_run() {
    let (world, mut state) = building();
    let branch = state.runs[&key()].branch.clone();

    world
        .pulls
        .borrow_mut()
        .insert(branch, PullRequest::Closed { number: 31 });
    world.poll(&world.project(), &mut state);

    assert_eq!(phase(&state), Phase::Blocked(Reason::Failed(1)));
    stopped(&world, "closed without merging");
}

/// Two commands after the worktree, and a fix when either fails.
fn audit() -> Vec<atelier_core::Settings> {
    vec![
        step("worktree", "git.worktree", &[]),
        step("deps", "atelier.command", &[]),
        step("lint", "atelier.command", &[("after", list(&["worktree"]))]),
        step(
            "fix",
            "atelier.command",
            &[(
                "after_any",
                table(&[("deps", "failed"), ("lint", "failed")]),
            )],
        ),
    ]
}

fn passed(why: &str) -> Observed {
    Observed::Settled(atelier_core::Outcome {
        name: "passed".into(),
        why: why.into(),
    })
}

#[test]
fn a_pipeline_without_forge_steps_does_not_ask_the_forge() {
    let world = World::with_issue("12", "Audit", &[Mark::Build]);
    let mut project = world.project_with(&audit());
    project.forge = "another-plugin".into();
    let mut state = State::default();

    world.poll(&project, &mut state);
    world.poll(&project, &mut state);

    assert_eq!(phase(&state), Phase::Running);
    assert_eq!(*world.forge_reads.borrow(), 0);
}

#[test]
fn steps_whose_needs_hold_together_start_in_the_same_poll() {
    let world = World::with_issue("12", "Audit", &[Mark::Build]);
    let project = world.project_with(&audit());
    let mut state = State::default();

    world.poll(&project, &mut state);

    assert_eq!(*world.started.borrow(), ["worktree", "deps", "lint"]);
    assert_eq!(status(&state, "deps"), Some(Status::Running));
    assert_eq!(status(&state, "lint"), Some(Status::Running));

    world.commands.borrow_mut().insert(
        "deps".into(),
        Observed::Failed {
            why: "two advisories".into(),
            last: false,
        },
    );
    world
        .commands
        .borrow_mut()
        .insert("lint".into(), passed("clean"));
    world.poll(&project, &mut state);

    assert_eq!(status(&state, "fix"), Some(Status::Running));
    assert_eq!(phase(&state), Phase::Running);

    world
        .commands
        .borrow_mut()
        .insert("fix".into(), passed("fixed"));
    world.poll(&project, &mut state);

    let ticket = world.ticket("12");
    let report = ticket.comments.last().unwrap();
    assert_eq!(phase(&state), Phase::Done);
    assert!(
        report.contains("- `deps`: failed (two advisories)"),
        "{report}"
    );
    assert!(report.contains("- `fix`: passed (fixed)"), "{report}");
    assert!(!ticket.open);
}

#[test]
fn a_step_whose_needs_cannot_hold_is_skipped_and_the_run_is_done() {
    let world = World::with_issue("12", "Audit", &[Mark::Build]);
    let project = world.project_with(&audit());
    let mut state = State::default();
    world
        .commands
        .borrow_mut()
        .insert("deps".into(), passed("no advisories"));
    world
        .commands
        .borrow_mut()
        .insert("lint".into(), passed("clean"));

    world.poll(&project, &mut state);

    let report = world.ticket("12").comments.last().unwrap().clone();
    assert_eq!(phase(&state), Phase::Done);
    assert_eq!(status(&state, "fix"), Some(Status::Skipped));
    assert!(report.contains("- `fix`: skipped"), "{report}");
    assert_eq!(*world.started.borrow(), ["worktree", "deps", "lint"]);
}

#[test]
fn a_command_that_settles_an_outcome_it_lacks_is_reported() {
    let world = World::with_issue("12", "Audit", &[Mark::Build]);
    let project = world.project_with(&audit());
    let mut state = State::default();
    world.commands.borrow_mut().insert(
        "deps".into(),
        Observed::Settled(atelier_core::Outcome {
            name: "found".into(),
            why: String::new(),
        }),
    );

    let report = world.poll(&project, &mut state);

    assert_eq!(phase(&state), Phase::Running);
    assert!(
        report.failures[0]
            .1
            .message
            .contains("deps settled found, which is none of its outcomes"),
        "{report:?}"
    );
}

#[test]
fn an_issue_marked_working_that_atelier_never_took_is_blocked() {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Working]);
    let mut state = State::default();

    world.poll(&world.project(), &mut state);

    let ticket = world.ticket("12");
    assert_eq!(phase(&state), Phase::Blocked(Reason::LostTrack));
    assert_eq!(ticket.marks, Marks::default().with(Mark::Blocked));
    assert!(ticket.comments[0].contains("no record"));
}

#[test]
fn an_unmarked_issue_is_left_alone() {
    let world = World::with_issue("12", "Fix the dead link", &[]);
    let mut state = State::default();

    let report = world.poll(&world.project(), &mut state);

    assert_eq!(report.moves, []);
    assert!(state.runs.is_empty());
    assert_eq!(*world.changes.borrow(), 0);
}

#[test]
fn a_finished_run_is_not_read_again() {
    let (world, mut state) = building();
    let project = world.project();
    let branch = state.runs[&key()].branch.clone();
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

/// A run of the build with the worktree passed and the builder running,
/// as a record would hold it.
fn running(steps: &[&str]) -> Record {
    let mut record = Record::new(&key(), "Fix the dead link", "build");
    let mut progress = Progress::default();
    progress.status[0] = Status::Settled(Outcome(0));
    progress.status[1] = Status::Running;
    record.phase = Phase::Running;
    record.progress = progress;
    record.steps = steps
        .iter()
        .map(|id| StepRecord {
            id: (*id).to_owned(),
            plugin: String::new(),
            session: None,
            why: None,
        })
        .collect();
    record
}

#[test]
fn a_builder_without_a_worktree_is_blocked_not_started() {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Working]);
    let mut state = State::default();
    let mut record = running(&["worktree", "builder", "checks", "ready", "merge"]);
    record.pending = vec![Action::Start(1)];
    state.runs.insert(key(), record);

    world.poll(&world.project(), &mut state);

    assert_eq!(phase(&state), Phase::Blocked(Reason::Start(1)));
    assert!(
        world.ticket("12").comments[0]
            .contains("could not start `builder` for this issue: the session has no worktree")
    );
    assert_eq!(world.sessions.borrow().len(), 0);
}

#[test]
fn a_run_whose_pipeline_changed_under_it_is_blocked() {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Working]);
    let mut state = State::default();
    state
        .runs
        .insert(key(), running(&["worktree", "builder", "review"]));

    world.poll(&world.project(), &mut state);

    assert_eq!(phase(&state), Phase::Blocked(Reason::Changed));
    assert!(world.ticket("12").comments[0].contains("its pipeline changed while it ran"));
}

#[test]
fn removing_a_pipeline_blocks_its_running_or_claiming_run() {
    for before in [Phase::Running, Phase::Claiming] {
        let world = World::with_issue("12", "Fix the dead link", &[Mark::Working]);
        let mut state = State::default();
        let mut record = running(&["worktree", "builder", "checks", "ready", "merge"]);
        record.phase = before;
        record.pipeline = "removed".into();
        state.runs.insert(key(), record);

        let report = world.poll(&world.project(), &mut state);

        assert_eq!(phase(&state), Phase::Blocked(Reason::Changed), "{before:?}");
        assert_eq!(report.failures, []);
    }
}

#[test]
fn saved_progress_the_pipeline_cannot_resume_blocks_its_run() {
    let world = World::with_issue("12", "Fix the dead link", &[Mark::Working]);
    let mut state = State::default();
    let mut record = running(&["worktree", "builder", "checks", "ready", "merge"]);
    record.progress.status[0] = Status::Pending;
    state.runs.insert(key(), record);

    world.poll(&world.project(), &mut state);

    assert_eq!(phase(&state), Phase::Blocked(Reason::Changed));
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

    world.poll(&world.project(), &mut state);

    let lost = IssueKey {
        tracker: "mem".into(),
        id: "13".into(),
    };
    assert!(!state.runs.contains_key(&key()));
    assert_eq!(state.runs[&lost].phase, Phase::Blocked(Reason::LostTrack));
}
