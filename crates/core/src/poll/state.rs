use std::collections::BTreeMap;
use std::path::PathBuf;

use rungs::Progress;

use crate::{Action, IssueKey, Mark, Phase, Pipeline, Run, Session};

/// What atelier keeps of a project between polls: each item's run and the
/// trackers' listing tags. The tracker's marks stay the source of truth.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub runs: BTreeMap<IssueKey, Record>,
    pub etags: BTreeMap<(String, Mark), String>,
}

/// One item's run of a pipeline; an item has one run at a time. `pending`
/// holds the actions of its last move not yet carried out, so a stop
/// between actions resumes where it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub pipeline: String,
    pub phase: Phase,
    /// Each step's status and failed rounds, by its position.
    pub progress: Progress,
    /// What each step keeps, in the pipeline's order.
    pub steps: Vec<StepRecord>,
    pub title: String,
    pub branch: String,
    pub worktree: Option<PathBuf>,
    /// The number of the pull request on its branch, once a poll saw one.
    pub pull_request: Option<u64>,
    pub pending: Vec<Action>,
}

/// What a run keeps of one step: its session, once a start made one, and
/// why it last settled or failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRecord {
    pub id: String,
    pub session: Option<Session>,
    pub why: Option<String>,
}

/// The longest branch slug, so a branch name stays readable.
const SLUG_CHARS: usize = 40;

impl Record {
    /// An untracked run of `pipeline`, on branch `issue-<id>-<slug>`.
    #[must_use]
    pub fn new(key: &IssueKey, title: &str, pipeline: &str) -> Self {
        Self {
            pipeline: pipeline.to_owned(),
            phase: Phase::Untracked,
            progress: Progress::default(),
            steps: Vec::new(),
            title: title.to_owned(),
            branch: branch(key, title),
            worktree: None,
            pull_request: None,
            pending: Vec::new(),
        }
    }

    /// Its phase and progress, as the machine moves them.
    #[must_use]
    pub const fn run(&self) -> Run {
        Run {
            phase: self.phase,
            progress: self.progress,
        }
    }

    /// Each session its steps made, in order.
    pub fn sessions(&self) -> impl Iterator<Item = &Session> {
        self.steps.iter().filter_map(|step| step.session.as_ref())
    }

    /// The id of the step at `index`, or its number.
    #[must_use]
    pub fn step_id(&self, index: u8) -> String {
        self.steps
            .get(usize::from(index))
            .map_or_else(|| format!("step {index}"), |step| step.id.clone())
    }

    /// Whether its steps are `pipeline`'s, in order.
    #[must_use]
    pub fn follows(&self, pipeline: &Pipeline) -> bool {
        self.steps
            .iter()
            .map(|step| step.id.as_str())
            .eq(pipeline.steps().iter().map(|step| step.id.as_str()))
    }

    /// Takes `pipeline`'s steps for a fresh run, keeping each step's last
    /// session, which its next start replaces.
    pub fn begin(&mut self, pipeline: &Pipeline) {
        let last = std::mem::take(&mut self.steps);

        pipeline.name().clone_into(&mut self.pipeline);
        self.steps = pipeline
            .steps()
            .iter()
            .map(|step| StepRecord {
                id: step.id.clone(),
                session: last
                    .iter()
                    .find(|kept| kept.id == step.id)
                    .and_then(|kept| kept.session.clone()),
                why: None,
            })
            .collect();
    }

    /// The worktree's folder name, `issue-<id>`.
    #[must_use]
    pub fn folder(key: &IssueKey) -> String {
        format!("issue-{}", slug(&key.id, usize::MAX))
    }
}

fn branch(key: &IssueKey, title: &str) -> String {
    let title = slug(title, SLUG_CHARS);

    if title.is_empty() {
        Record::folder(key)
    } else {
        format!("{}-{title}", Record::folder(key))
    }
}

/// Lowercase letters and digits, runs of anything else as one `-`.
fn slug(text: &str, limit: usize) -> String {
    let words: Vec<String> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();

    let mut slug = String::new();

    for word in words {
        let extra = usize::from(!slug.is_empty()).saturating_add(word.len());

        if slug.len().saturating_add(extra) > limit {
            break;
        }

        if !slug.is_empty() {
            slug.push('-');
        }

        slug.push_str(&word);
    }

    slug
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(tracker: &str, id: &str) -> IssueKey {
        IssueKey {
            tracker: tracker.into(),
            id: id.into(),
        }
    }

    #[test]
    fn a_branch_names_the_issue_and_its_title_in_at_most_forty_characters() {
        let cases = [
            (
                key("github", "12"),
                "Fix the dead link",
                "issue-12-fix-the-dead-link",
            ),
            (
                key("jira", "CHF-12"),
                "Fix: the *dead* link!",
                "issue-chf-12-fix-the-dead-link",
            ),
            (key("github", "7"), "¿?", "issue-7"),
            (
                key("github", "9"),
                "one two three four five six seven eight nine",
                "issue-9-one-two-three-four-five-six-seven-eight",
            ),
            (key("github", "3"), &"a".repeat(41), "issue-3"),
            (
                key("github", "4"),
                &"a".repeat(40),
                &format!("issue-4-{}", "a".repeat(40)),
            ),
        ];

        for (key, title, branch) in cases {
            assert_eq!(Record::new(&key, title, "build").branch, branch, "{title}");
        }

        assert_eq!(Record::folder(&key("jira", "CHF-12")), "issue-chf-12");
        assert_eq!(key("github", "12").to_string(), "github#12");
    }
}
