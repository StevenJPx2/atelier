//! `~/.local/state/atelier/state.json`: each project's issues and listing
//! tags. Written to a temporary file and renamed, so a stop never leaves it
//! half written.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use atelier_core::{Action, IssueKey, Mark, Phase, Reason, Record, Session, State};
use color_eyre::eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct File {
    projects: BTreeMap<String, ProjectFile>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ProjectFile {
    issues: Vec<IssueFile>,
    etags: Vec<EtagFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct IssueFile {
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
    Preparing,
    Starting,
    Building,
    Open { rounds: u8 },
    Stage { index: u8, rounds: u8 },
    Green { rounds: u8 },
    Ready { rounds: u8 },
    Done,
    Blocked { reason: ReasonFile },
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "name")]
enum ReasonFile {
    Start,
    RedRounds,
    Stage { index: u8 },
    Closed,
    LostTrack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
enum ActionFile {
    Mark { mark: MarkFile },
    Unmark { mark: MarkFile },
    CreateWorktree,
    StartBuilder,
    CommentSession,
    StartStage { index: u8 },
    Interrupt,
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
        let issues = file.issues.into_iter().map(|issue| {
            let key = IssueKey {
                tracker: issue.tracker,
                id: issue.id,
            };
            let record = Record {
                phase: issue.phase.into(),
                title: issue.title,
                branch: issue.branch,
                worktree: issue.worktree,
                session: issue.session.map(|session| Session {
                    id: session.id,
                    title: session.title,
                }),
                pull_request: issue.pull_request,
                pending: issue.pending.into_iter().map(Action::from).collect(),
            };
            (key, record)
        });
        let etags = file
            .etags
            .into_iter()
            .map(|tag| ((tag.tracker, tag.mark.into()), tag.etag));

        Self {
            issues: issues.collect(),
            etags: etags.collect(),
        }
    }
}

impl From<&State> for ProjectFile {
    fn from(state: &State) -> Self {
        let issues = state.issues.iter().map(|(key, record)| IssueFile {
            tracker: key.tracker.clone(),
            id: key.id.clone(),
            phase: record.phase.into(),
            title: record.title.clone(),
            branch: record.branch.clone(),
            worktree: record.worktree.clone(),
            session: record.session.as_ref().map(|session| SessionFile {
                id: session.id.clone(),
                title: session.title.clone(),
            }),
            pull_request: record.pull_request,
            pending: record
                .pending
                .iter()
                .copied()
                .map(ActionFile::from)
                .collect(),
        });
        let etags = state.etags.iter().map(|((tracker, mark), etag)| EtagFile {
            tracker: tracker.clone(),
            mark: (*mark).into(),
            etag: etag.clone(),
        });

        Self {
            issues: issues.collect(),
            etags: etags.collect(),
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
            ReasonFile::Start => Self::Start,
            ReasonFile::RedRounds => Self::RedRounds,
            ReasonFile::Stage { index } => Self::Stage(index),
            ReasonFile::Closed => Self::Closed,
            ReasonFile::LostTrack => Self::LostTrack,
        }
    }
}

impl From<Reason> for ReasonFile {
    fn from(reason: Reason) -> Self {
        match reason {
            Reason::Start => Self::Start,
            Reason::RedRounds => Self::RedRounds,
            Reason::Stage(index) => Self::Stage { index },
            Reason::Closed => Self::Closed,
            Reason::LostTrack => Self::LostTrack,
        }
    }
}

impl From<PhaseFile> for Phase {
    fn from(phase: PhaseFile) -> Self {
        match phase {
            PhaseFile::Untracked => Self::Untracked,
            PhaseFile::Claiming => Self::Claiming,
            PhaseFile::Preparing => Self::Preparing,
            PhaseFile::Starting => Self::Starting,
            PhaseFile::Building => Self::Building,
            PhaseFile::Open { rounds } => Self::Open { rounds },
            PhaseFile::Stage { index, rounds } => Self::Stage { index, rounds },
            PhaseFile::Green { rounds } => Self::Green { rounds },
            PhaseFile::Ready { rounds } => Self::Ready { rounds },
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
            Phase::Preparing => Self::Preparing,
            Phase::Starting => Self::Starting,
            Phase::Building => Self::Building,
            Phase::Open { rounds } => Self::Open { rounds },
            Phase::Stage { index, rounds } => Self::Stage { index, rounds },
            Phase::Green { rounds } => Self::Green { rounds },
            Phase::Ready { rounds } => Self::Ready { rounds },
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
            ActionFile::CreateWorktree => Self::CreateWorktree,
            ActionFile::StartBuilder => Self::StartBuilder,
            ActionFile::CommentSession => Self::CommentSession,
            ActionFile::StartStage { index } => Self::StartStage(index),
            ActionFile::Interrupt => Self::Interrupt,
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
            Action::CreateWorktree => Self::CreateWorktree,
            Action::StartBuilder => Self::StartBuilder,
            Action::CommentSession => Self::CommentSession,
            Action::StartStage(index) => Self::StartStage { index },
            Action::Interrupt => Self::Interrupt,
            Action::CloseIssue => Self::CloseIssue,
            Action::RemoveWorktree => Self::RemoveWorktree,
            Action::Comment(reason) => Self::Comment {
                reason: reason.into(),
            },
        }
    }
}

/// The phase as `atelier status` shows it, such as `Blocked(RedRounds)`.
pub fn shown(phase: Phase) -> String {
    format!("{phase:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_phase_and_action_survives_the_file() {
        let phases = [
            Phase::Untracked,
            Phase::Claiming,
            Phase::Preparing,
            Phase::Starting,
            Phase::Building,
            Phase::Open { rounds: 2 },
            Phase::Stage {
                index: 1,
                rounds: 3,
            },
            Phase::Green { rounds: 4 },
            Phase::Ready { rounds: 1 },
            Phase::Done,
            Phase::Blocked(Reason::Start),
            Phase::Blocked(Reason::RedRounds),
            Phase::Blocked(Reason::Stage(2)),
            Phase::Blocked(Reason::Closed),
            Phase::Blocked(Reason::LostTrack),
            Phase::Stopped,
        ];
        let pending = vec![
            Action::Mark(Mark::Build),
            Action::Unmark(Mark::Working),
            Action::Mark(Mark::Blocked),
            Action::CreateWorktree,
            Action::StartBuilder,
            Action::CommentSession,
            Action::StartStage(1),
            Action::Interrupt,
            Action::CloseIssue,
            Action::RemoveWorktree,
            Action::Comment(Reason::Stage(1)),
        ];
        let mut state = State::default();

        for (number, phase) in phases.into_iter().enumerate() {
            let key = IssueKey {
                tracker: "github".into(),
                id: number.to_string(),
            };
            let mut record = Record::new(&key, "Fix the dead link");
            record.phase = phase;
            record.pending.clone_from(&pending);
            record.worktree = Some(PathBuf::from("/w"));
            record.session = Some(Session {
                id: "ses_1".into(),
                title: "#1".into(),
            });
            record.pull_request = Some(8);
            state.issues.insert(key, record);
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
