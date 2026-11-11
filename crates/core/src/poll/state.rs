use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::{Action, IssueKey, Mark, Phase, Session};

/// What atelier keeps of a project between polls: each issue it took and
/// the trackers' listing tags. The tracker's marks stay the source of truth.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub issues: BTreeMap<IssueKey, Record>,
    pub etags: BTreeMap<(String, Mark), String>,
}

/// One issue atelier took. `pending` holds the actions of its last step not
/// yet carried out, so a stop between actions resumes where it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub phase: Phase,
    pub title: String,
    pub branch: String,
    pub worktree: Option<PathBuf>,
    pub session: Option<Session>,
    pub pending: Vec<Action>,
}

/// The longest branch slug, so a branch name stays readable.
const SLUG_CHARS: usize = 40;

impl Record {
    /// An untracked issue's record, on branch `issue-<id>-<slug>`.
    #[must_use]
    pub fn new(key: &IssueKey, title: &str) -> Self {
        Self {
            phase: Phase::Untracked,
            title: title.to_owned(),
            branch: branch(key, title),
            worktree: None,
            session: None,
            pending: Vec::new(),
        }
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
            assert_eq!(Record::new(&key, title).branch, branch, "{title}");
        }

        assert_eq!(Record::folder(&key("jira", "CHF-12")), "issue-chf-12");
        assert_eq!(key("github", "12").to_string(), "github#12");
    }
}
