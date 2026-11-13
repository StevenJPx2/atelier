//! The builder's brief: everything it is told when it starts.

use crate::Issue;

const CONTRACT: &str = include_str!("../prompts/builder.md");

/// Commands no builder runs, whatever its forge.
const DENIED: [&str; 2] = ["git tag *", "git push * --tags"];

/// The brief core puts together; the agent plugin renders it for its harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brief {
    pub title: String,
    pub branch: String,
    pub parts: Vec<String>,
    pub denied: Vec<String>,
}

impl Brief {
    /// The contract, filled in for one issue.
    ///
    /// ```
    /// use atelier_core::{Brief, Issue, IssueKey, Marks};
    ///
    /// let issue = Issue {
    ///     key: IssueKey { tracker: "github".into(), id: "12".into() },
    ///     title: "Fix the dead link".into(),
    ///     open: true,
    ///     marks: Marks::default(),
    /// };
    ///
    /// let brief = Brief::new("#12", &issue, "issue-12-fix-the-dead-link", "v2")
    ///     .with("Open the pull request with gh pr create --draft.".into())
    ///     .deny(["gh pr merge *".into()]);
    ///
    /// assert_eq!(brief.title, "#12 Fix the dead link");
    /// assert!(brief.text().contains("into `v2`"));
    /// assert!(brief.denied.contains(&"git tag *".to_owned()));
    /// ```
    #[must_use]
    pub fn new(reference: &str, issue: &Issue, branch: &str, base: &str) -> Self {
        let contract = CONTRACT
            .replace("$reference", reference)
            .replace("$title", &issue.title)
            .replace("$branch", branch)
            .replace("$base", base);

        Self {
            title: format!("{reference} {}", issue.title),
            branch: branch.to_owned(),
            parts: vec![contract],
            denied: DENIED.iter().map(|&pattern| pattern.to_owned()).collect(),
        }
    }

    /// The brief of an agent's step with its own prompt: the prompt in
    /// place of every part, with the same title, branch and denials.
    #[must_use]
    pub fn prompted(&self, prompt: &str) -> Self {
        Self {
            parts: vec![prompt.to_owned()],
            ..self.clone()
        }
    }

    #[must_use]
    pub fn with(mut self, part: String) -> Self {
        self.parts.push(part);
        self
    }

    #[must_use]
    pub fn deny(mut self, patterns: impl IntoIterator<Item = String>) -> Self {
        self.denied.extend(patterns);
        self
    }

    /// The parts as one text, in order.
    #[must_use]
    pub fn text(&self) -> String {
        self.parts.join("\n\n")
    }
}
