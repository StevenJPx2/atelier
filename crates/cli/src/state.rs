//! `~/.local/state/atelier/state.json`: each project's runs and listing
//! tags. Written to a temporary file and renamed, so a stop never leaves it
//! half written.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use atelier_core::rungs::{MAX_STEPS, Outcome, Progress, Status};
use atelier_core::{Action, IssueKey, Mark, Phase, Reason, Record, Session, State, StepRecord};
use color_eyre::eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct File {
    projects: BTreeMap<String, ProjectFile>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ProjectFile {
    #[serde(default)]
    runs: Vec<RunFile>,
    etags: Vec<EtagFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RunFile {
    pipeline: String,
    tracker: String,
    id: String,
    phase: PhaseFile,
    steps: Vec<StepFile>,
    title: String,
    branch: String,
    worktree: Option<PathBuf>,
    pull_request: Option<u64>,
    pending: Vec<ActionFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StepFile {
    id: String,
    status: StatusFile,
    failed: u8,
    session: Option<SessionFile>,
    why: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "name")]
enum StatusFile {
    Pending,
    Running,
    Settled { outcome: u8 },
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SessionFile {
    id: String,
    title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EtagFile {
    tracker: String,
    mark: MarkFile,
    etag: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MarkFile {
    Build,
    Working,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "name")]
enum PhaseFile {
    Untracked,
    Claiming,
    Running,
    Done,
    Blocked { reason: ReasonFile },
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "name")]
enum ReasonFile {
    Claim,
    Start { step: u8 },
    Failed { step: u8 },
    Changed,
    LostTrack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
enum ActionFile {
    Mark { mark: MarkFile },
    Unmark { mark: MarkFile },
    Start { step: u8 },
    Announce { step: u8 },
    Interrupt,
    Report,
    CloseIssue,
    RemoveWorktree,
    Comment { reason: ReasonFile },
}

/// Every project's state; none for a file not written yet.
///
/// # Errors
///
/// The file cannot be read or is not valid.
pub fn load(path: &Path) -> Result<BTreeMap<String, State>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error).wrap_err_with(|| format!("reading {}", path.display())),
    };

    let file: File =
        serde_json::from_str(&text).wrap_err_with(|| format!("reading {}", path.display()))?;

    Ok(file
        .projects
        .into_iter()
        .map(|(repo, project)| (repo, project.into()))
        .collect())
}

/// Writes every project's state at once.
///
/// # Errors
///
/// The folder or the file cannot be written.
pub fn write(path: &Path, projects: &BTreeMap<String, State>) -> Result<()> {
    let file = File {
        projects: projects
            .iter()
            .map(|(repo, state)| (repo.clone(), state.into()))
            .collect(),
    };

    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }

    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(&file)?)?;
    fs::rename(&temporary, path).wrap_err_with(|| format!("writing {}", path.display()))?;

    Ok(())
}

impl From<ProjectFile> for State {
    fn from(file: ProjectFile) -> Self {
        let runs = file.runs.into_iter().map(RunFile::into_record);
        let etags = file
            .etags
            .into_iter()
            .map(|tag| ((tag.tracker, tag.mark.into()), tag.etag));

        Self {
            runs: runs.collect(),
            etags: etags.collect(),
        }
    }
}

impl From<&State> for ProjectFile {
    fn from(state: &State) -> Self {
        let runs = state
            .runs
            .iter()
            .map(|(key, record)| RunFile::from_record(key, record));
        let etags = state.etags.iter().map(|((tracker, mark), etag)| EtagFile {
            tracker: tracker.clone(),
            mark: (*mark).into(),
            etag: etag.clone(),
        });

        Self {
            runs: runs.collect(),
            etags: etags.collect(),
        }
    }
}

impl RunFile {
    fn into_record(self) -> (IssueKey, Record) {
        let key = IssueKey {
            tracker: self.tracker,
            id: self.id,
        };
        let mut progress = Progress::default();
        let positions = progress.status.iter_mut().zip(progress.failed.iter_mut());

        for ((status, failed), step) in positions.zip(&self.steps) {
            *status = step.status.into();
            *failed = step.failed;
        }

        let steps = self
            .steps
            .into_iter()
            .take(MAX_STEPS)
            .map(|step| StepRecord {
                id: step.id,
                session: step.session.map(|session| Session {
                    id: session.id,
                    title: session.title,
                }),
                why: step.why,
            });

        let record = Record {
            pipeline: self.pipeline,
            phase: self.phase.into(),
            progress,
            steps: steps.collect(),
            title: self.title,
            branch: self.branch,
            worktree: self.worktree,
            pull_request: self.pull_request,
            pending: self.pending.into_iter().map(Action::from).collect(),
        };

        (key, record)
    }

    fn from_record(key: &IssueKey, record: &Record) -> Self {
        let progress = record.progress.status.iter().zip(record.progress.failed);
        let steps = record
            .steps
            .iter()
            .zip(progress)
            .map(|(step, (status, failed))| StepFile {
                id: step.id.clone(),
                status: (*status).into(),
                failed,
                session: step.session.as_ref().map(|session| SessionFile {
                    id: session.id.clone(),
                    title: session.title.clone(),
                }),
                why: step.why.clone(),
            });

        Self {
            pipeline: record.pipeline.clone(),
            tracker: key.tracker.clone(),
            id: key.id.clone(),
            phase: record.phase.into(),
            steps: steps.collect(),
            title: record.title.clone(),
            branch: record.branch.clone(),
            worktree: record.worktree.clone(),
            pull_request: record.pull_request,
            pending: record
                .pending
                .iter()
                .copied()
                .map(ActionFile::from)
                .collect(),
        }
    }
}

impl From<StatusFile> for Status {
    fn from(status: StatusFile) -> Self {
        match status {
            StatusFile::Pending => Self::Pending,
            StatusFile::Running => Self::Running,
            StatusFile::Settled { outcome } => Self::Settled(Outcome(outcome)),
            StatusFile::Skipped => Self::Skipped,
        }
    }
}

impl From<Status> for StatusFile {
    fn from(status: Status) -> Self {
        match status {
            Status::Pending => Self::Pending,
            Status::Running => Self::Running,
            Status::Settled(outcome) => Self::Settled { outcome: outcome.0 },
            Status::Skipped => Self::Skipped,
        }
    }
}

impl From<MarkFile> for Mark {
    fn from(mark: MarkFile) -> Self {
        match mark {
            MarkFile::Build => Self::Build,
            MarkFile::Working => Self::Working,
            MarkFile::Blocked => Self::Blocked,
        }
    }
}

impl From<Mark> for MarkFile {
    fn from(mark: Mark) -> Self {
        match mark {
            Mark::Build => Self::Build,
            Mark::Working => Self::Working,
            Mark::Blocked => Self::Blocked,
        }
    }
}

impl From<ReasonFile> for Reason {
    fn from(reason: ReasonFile) -> Self {
        match reason {
            ReasonFile::Claim => Self::Claim,
            ReasonFile::Start { step } => Self::Start(step),
            ReasonFile::Failed { step } => Self::Failed(step),
            ReasonFile::Changed => Self::Changed,
            ReasonFile::LostTrack => Self::LostTrack,
        }
    }
}

impl From<Reason> for ReasonFile {
    fn from(reason: Reason) -> Self {
        match reason {
            Reason::Claim => Self::Claim,
            Reason::Start(step) => Self::Start { step },
            Reason::Failed(step) => Self::Failed { step },
            Reason::Changed => Self::Changed,
            Reason::LostTrack => Self::LostTrack,
        }
    }
}

impl From<PhaseFile> for Phase {
    fn from(phase: PhaseFile) -> Self {
        match phase {
            PhaseFile::Untracked => Self::Untracked,
            PhaseFile::Claiming => Self::Claiming,
            PhaseFile::Running => Self::Running,
            PhaseFile::Done => Self::Done,
            PhaseFile::Blocked { reason } => Self::Blocked(reason.into()),
            PhaseFile::Stopped => Self::Stopped,
        }
    }
}

impl From<Phase> for PhaseFile {
    fn from(phase: Phase) -> Self {
        match phase {
            Phase::Untracked => Self::Untracked,
            Phase::Claiming => Self::Claiming,
            Phase::Running => Self::Running,
            Phase::Done => Self::Done,
            Phase::Blocked(reason) => Self::Blocked {
                reason: reason.into(),
            },
            Phase::Stopped => Self::Stopped,
        }
    }
}

impl From<ActionFile> for Action {
    fn from(action: ActionFile) -> Self {
        match action {
            ActionFile::Mark { mark } => Self::Mark(mark.into()),
            ActionFile::Unmark { mark } => Self::Unmark(mark.into()),
            ActionFile::Start { step } => Self::Start(step),
            ActionFile::Announce { step } => Self::Announce(step),
            ActionFile::Interrupt => Self::Interrupt,
            ActionFile::Report => Self::Report,
            ActionFile::CloseIssue => Self::CloseIssue,
            ActionFile::RemoveWorktree => Self::RemoveWorktree,
            ActionFile::Comment { reason } => Self::Comment(reason.into()),
        }
    }
}

impl From<Action> for ActionFile {
    fn from(action: Action) -> Self {
        match action {
            Action::Mark(mark) => Self::Mark { mark: mark.into() },
            Action::Unmark(mark) => Self::Unmark { mark: mark.into() },
            Action::Start(step) => Self::Start { step },
            Action::Announce(step) => Self::Announce { step },
            Action::Interrupt => Self::Interrupt,
            Action::Report => Self::Report,
            Action::CloseIssue => Self::CloseIssue,
            Action::RemoveWorktree => Self::RemoveWorktree,
            Action::Comment(reason) => Self::Comment {
                reason: reason.into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_phase_status_and_action_survives_the_file() {
        let phases = [
            Phase::Untracked,
            Phase::Claiming,
            Phase::Running,
            Phase::Done,
            Phase::Blocked(Reason::Claim),
            Phase::Blocked(Reason::Start(1)),
            Phase::Blocked(Reason::Failed(2)),
            Phase::Blocked(Reason::Changed),
            Phase::Blocked(Reason::LostTrack),
            Phase::Stopped,
        ];
        let pending = vec![
            Action::Mark(Mark::Build),
            Action::Unmark(Mark::Working),
            Action::Mark(Mark::Blocked),
            Action::Start(0),
            Action::Announce(1),
            Action::Interrupt,
            Action::Report,
            Action::CloseIssue,
            Action::RemoveWorktree,
            Action::Comment(Reason::Failed(2)),
        ];
        let mut progress = Progress::default();
        progress.status[0] = Status::Settled(Outcome(0));
        progress.status[1] = Status::Running;
        progress.status[2] = Status::Skipped;
        progress.status[3] = Status::Settled(Outcome::FAILED);
        progress.failed[3] = 2;
        let steps = ["worktree", "builder", "checks", "ready"].map(|id| StepRecord {
            id: id.into(),
            session: None,
            why: None,
        });
        let mut state = State::default();

        for (number, phase) in phases.into_iter().enumerate() {
            let key = IssueKey {
                tracker: "github".into(),
                id: number.to_string(),
            };
            let mut record = Record::new(&key, "Fix the dead link", "build");
            record.phase = phase;
            record.progress = progress;
            record.steps = steps.to_vec();
            record.steps[1].session = Some(Session {
                id: "ses_1".into(),
                title: "#1".into(),
            });
            record.steps[3].why = Some("CI failed on 2 commits".into());
            record.pending.clone_from(&pending);
            record.worktree = Some(PathBuf::from("/w"));
            record.pull_request = Some(8);
            state.runs.insert(key, record);
        }
        state
            .etags
            .insert(("github".into(), Mark::Working), "W/\"tag\"".into());

        let folder = std::env::temp_dir().join(format!("atelier-state-{}", std::process::id()));
        let path = folder.join("state.json");
        let projects = BTreeMap::from([("StevenJPx2/demo".to_owned(), state)]);
        write(&path, &projects).unwrap();
        let read = load(&path);
        let unreadable = load(&folder);
        let _ = fs::remove_dir_all(&folder);

        assert_eq!(read.unwrap(), projects);
        assert!(unreadable.is_err());
        assert_eq!(load(&folder.join("missing.json")).unwrap(), BTreeMap::new());
    }
}
