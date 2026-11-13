//! `state.json` as `0.1.0-rc.8` and earlier wrote it: each issue's phase in
//! the loop the built-in `build` pipeline now writes out, read into a run of
//! it where the issue was.

use std::path::PathBuf;

use atelier_core::rungs::{Outcome, Progress, Status};
use atelier_core::{Action, IssueKey, Mark, Phase, Reason, Record, Session, StepRecord};
use serde::Deserialize;

use super::{MarkFile, SessionFile};

/// The built-in `build`'s steps, by position.
const BUILD: [&str; 5] = ["worktree", "builder", "checks", "ready", "merge"];

const WORKTREE: u8 = 0;
const BUILDER: u8 = 1;
const CHECKS: u8 = 2;
const READY: u8 = 3;
const MERGE: u8 = 4;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct IssueFile {
    tracker: String,
    id: String,
    phase: PhaseFile,
    title: String,
    branch: String,
    worktree: Option<PathBuf>,
    session: Option<SessionFile>,
    #[serde(default)]
    pull_request: Option<u64>,
    pending: Vec<ActionFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", tag = "name")]
enum PhaseFile {
    Untracked,
    Claiming,
    Preparing,
    Starting,
    Building,
    Open { rounds: u8 },
    Stage { rounds: u8 },
    Green { rounds: u8 },
    Ready { rounds: u8 },
    Done,
    Blocked { reason: ReasonFile },
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", tag = "name")]
enum ReasonFile {
    Start,
    RedRounds,
    Stage,
    Closed,
    LostTrack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
enum ActionFile {
    Mark { mark: MarkFile },
    Unmark { mark: MarkFile },
    CreateWorktree,
    StartBuilder,
    CommentSession,
    StartStage,
    Interrupt,
    TellGreen,
    CloseIssue,
    RemoveWorktree,
    Comment { reason: ReasonFile },
}

impl IssueFile {
    /// The issue's run of `build`: Building is `builder` running; Open is
    /// `builder` settled `opened` and `checks` running, with its failed
    /// rounds; Green is `ready` running; and Ready is `merge` running.
    pub fn into_record(self) -> (IssueKey, Record) {
        let key = IssueKey {
            tracker: self.tracker,
            id: self.id,
        };
        let (phase, progress) = self.phase.run();
        let steps = BUILD.map(|id| StepRecord {
            id: id.to_owned(),
            session: None,
            why: None,
        });
        let mut record = Record::new(&key, &self.title, "build");

        record.phase = phase;
        record.progress = progress;
        record.steps = steps.to_vec();
        record.branch = self.branch;
        record.worktree = self.worktree;
        record.pull_request = self.pull_request;
        record.pending = self
            .pending
            .into_iter()
            .filter_map(ActionFile::action)
            .collect();

        if let Some(builder) = record.steps.get_mut(usize::from(BUILDER)) {
            builder.session = self.session.map(|session| Session {
                id: session.id,
                title: session.title,
            });
        }

        (key, record)
    }
}

impl PhaseFile {
    fn run(self) -> (Phase, Progress) {
        let passed = |through: u8, rounds: u8, running: bool| {
            let mut progress = Progress::default();
            let steps = progress.status.iter_mut().take(usize::from(through));
            steps.for_each(|status| *status = Status::Settled(Outcome(0)));

            if let (true, Some(next)) = (running, progress.status.get_mut(usize::from(through))) {
                *next = Status::Running;
            }

            if let Some(failed) = progress.failed.get_mut(usize::from(CHECKS)) {
                *failed = rounds;
            }

            progress
        };

        match self {
            Self::Untracked => (Phase::Untracked, Progress::default()),
            Self::Claiming => (Phase::Claiming, Progress::default()),
            Self::Preparing => (Phase::Running, passed(WORKTREE, 0, true)),
            Self::Starting | Self::Building => (Phase::Running, passed(BUILDER, 0, true)),
            Self::Open { rounds } | Self::Stage { rounds } => {
                (Phase::Running, passed(CHECKS, rounds, true))
            }
            Self::Green { rounds } => (Phase::Running, passed(READY, rounds, true)),
            Self::Ready { rounds } => (Phase::Running, passed(MERGE, rounds, true)),
            Self::Done => (Phase::Done, passed(MERGE.saturating_add(1), 0, false)),
            Self::Blocked { reason } => (Phase::Blocked(reason.into()), Progress::default()),
            Self::Stopped => (Phase::Stopped, Progress::default()),
        }
    }
}

impl From<ReasonFile> for Reason {
    fn from(reason: ReasonFile) -> Self {
        match reason {
            ReasonFile::Start => Self::Start(BUILDER),
            ReasonFile::RedRounds | ReasonFile::Stage => Self::Failed(CHECKS),
            ReasonFile::Closed => Self::Failed(MERGE),
            ReasonFile::LostTrack => Self::LostTrack,
        }
    }
}

impl ActionFile {
    /// The action as the run takes it; a stage, which no plugin ran, has
    /// none.
    fn action(self) -> Option<Action> {
        Some(match self {
            Self::Mark { mark } => Action::Mark(Mark::from(mark)),
            Self::Unmark { mark } => Action::Unmark(Mark::from(mark)),
            Self::CreateWorktree => Action::Start(WORKTREE),
            Self::StartBuilder => Action::Start(BUILDER),
            Self::CommentSession => Action::Announce(BUILDER),
            Self::StartStage => return None,
            Self::Interrupt => Action::Interrupt,
            Self::TellGreen => Action::Start(READY),
            Self::CloseIssue => Action::CloseIssue,
            Self::RemoveWorktree => Action::RemoveWorktree,
            Self::Comment { reason } => Action::Comment(reason.into()),
        })
    }
}
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use atelier_core::{Record, State};

    use super::*;

    /// `state.json` as `0.1.0-rc.8` wrote it, an issue in each phase.
    const RC8: &str = include_str!("fixtures/rc8.json");

    fn read() -> State {
        let file: super::super::File = serde_json::from_str(RC8).unwrap();
        let mut projects: BTreeMap<String, State> = file
            .projects
            .into_iter()
            .map(|(repo, project)| (repo, project.into()))
            .collect();

        projects.remove("StevenJPx2/atelier").unwrap()
    }

    fn run(state: &State, id: &str) -> Record {
        let key = IssueKey {
            tracker: "github".into(),
            id: id.into(),
        };

        state.runs[&key].clone()
    }

    const P: Status = Status::Pending;
    const R: Status = Status::Running;
    const OK: Status = Status::Settled(Outcome(0));

    #[test]
    fn each_issue_is_a_run_of_build_where_it_was() {
        let state = read();
        let statuses = |id| run(&state, id).progress.status[..5].to_vec();

        assert_eq!(state.runs.len(), 11);
        assert_eq!(state.etags.len(), 1);
        assert_eq!(statuses("11"), [R, P, P, P, P]);
        assert_eq!(statuses("12"), [OK, R, P, P, P]);
        assert_eq!(statuses("13"), [OK, OK, R, P, P]);
        assert_eq!(statuses("14"), [OK, OK, R, P, P]);
        assert_eq!(statuses("21"), [OK, OK, OK, R, P]);
        assert_eq!(statuses("22"), [OK, OK, OK, OK, R]);
        assert_eq!(statuses("23"), [OK, OK, OK, OK, OK]);
        assert_eq!(run(&state, "13").progress.failed[2], 2);
        assert_eq!(run(&state, "22").progress.failed[2], 1);
        assert_eq!(run(&state, "12").phase, Phase::Running);
        assert_eq!(run(&state, "23").phase, Phase::Done);
        assert_eq!(
            run(&state, "24").phase,
            Phase::Blocked(Reason::Failed(CHECKS))
        );
        assert_eq!(run(&state, "25").phase, Phase::Stopped);
        assert_eq!(
            run(&state, "27").phase,
            Phase::Blocked(Reason::Failed(CHECKS))
        );
        assert_eq!(run(&state, "27").pull_request, None);
        assert_eq!(run(&state, "26").phase, Phase::Claiming);
    }

    #[test]
    fn each_run_keeps_its_builder_its_branch_and_its_pending_actions() {
        let state = read();
        let building = run(&state, "12");
        let builder = &building.steps[1];

        assert_eq!(building.pipeline, "build");
        assert_eq!(building.title, "Issue 12");
        assert_eq!(building.branch, "issue-12");
        assert_eq!(
            building.worktree,
            Some(PathBuf::from("/worktrees/issue-12"))
        );
        assert_eq!(builder.id, "builder");
        assert_eq!(builder.session.as_ref().unwrap().id, "ses_12");
        assert_eq!(run(&state, "21").pull_request, Some(8));

        let pending = |id| run(&state, id).pending;
        assert_eq!(pending("11"), [Action::Start(WORKTREE)]);
        assert_eq!(pending("12"), [Action::Announce(BUILDER)]);
        assert_eq!(pending("14"), []);
        assert_eq!(pending("21"), [Action::Start(READY)]);
        assert_eq!(
            pending("23"),
            [
                Action::CloseIssue,
                Action::Unmark(Mark::Working),
                Action::RemoveWorktree
            ]
        );
        assert_eq!(
            pending("24"),
            [Action::Interrupt, Action::Comment(Reason::Failed(CHECKS))]
        );
        assert_eq!(
            pending("26"),
            [Action::Mark(Mark::Working), Action::Unmark(Mark::Build)]
        );
    }

    #[test]
    fn every_reason_reads_as_the_step_it_stopped_at() {
        let reasons = [
            (ReasonFile::Start, Reason::Start(BUILDER)),
            (ReasonFile::RedRounds, Reason::Failed(CHECKS)),
            (ReasonFile::Stage, Reason::Failed(CHECKS)),
            (ReasonFile::Closed, Reason::Failed(MERGE)),
            (ReasonFile::LostTrack, Reason::LostTrack),
        ];

        for (file, reason) in reasons {
            assert_eq!(Reason::from(file), reason);
        }

        assert_eq!(
            ActionFile::Mark {
                mark: MarkFile::Blocked
            }
            .action(),
            Some(Action::Mark(Mark::Blocked))
        );
        assert_eq!(
            ActionFile::StartBuilder.action(),
            Some(Action::Start(BUILDER))
        );
    }
}
