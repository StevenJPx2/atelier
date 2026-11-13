//! A brief's denials as a session's permission rules.

use atelier_core::Brief;
use serde::Serialize;

/// What a rule does with a matching call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Allow,
    Deny,
}

/// One permission rule; the last rule that matches a call wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rule {
    pub action: String,
    pub resource: String,
    pub effect: Effect,
}

/// The tools a builder uses without asking anyone.
const ALLOWED: [&str; 12] = [
    "read",
    "list",
    "glob",
    "grep",
    "edit",
    "write",
    "patch",
    "shell",
    "skill",
    "task",
    "webfetch",
    "websearch",
];

fn rule(action: &str, resource: &str, effect: Effect) -> Rule {
    Rule {
        action: action.to_owned(),
        resource: resource.to_owned(),
        effect,
    }
}

/// The rules for a builder on `brief`, so it works without anyone answering.
///
/// Its tools are allowed. Every push is denied, then a push to its own
/// branch allowed; the brief's denied commands come after, so they win. The
/// question tool is denied: the builder writes on the pull request instead of
/// waiting for an answer.
///
/// ```
/// use atelier_core::{Brief, Issue, IssueKey, Marks, Limits};
/// use atelier_opencode::{Effect, Rule, rules};
///
/// let issue = Issue {
///     key: IssueKey { tracker: "github".into(), id: "12".into() },
///     title: "Fix the dead link".into(),
///     open: true,
///     marks: Marks::default(),
/// };
/// let brief = Brief::new("#12", &issue, "issue-12-fix", "v2", &Limits::new(5, [])?)
///     .deny(["gh pr merge *".into()]);
///
/// let rules = rules(&brief);
///
/// let shell = |resource: &str, effect| Rule { action: "shell".into(), resource: resource.into(), effect };
/// assert!(rules.contains(&shell("*", Effect::Allow)));
/// assert_eq!(rules[12..], [
///     shell("git push *", Effect::Deny),
///     shell("git push origin issue-12-fix", Effect::Allow),
///     shell("git push -u origin issue-12-fix", Effect::Allow),
///     shell("git push --set-upstream origin issue-12-fix", Effect::Allow),
///     shell("git tag *", Effect::Deny),
///     shell("git push * --tags", Effect::Deny),
///     shell("gh pr merge *", Effect::Deny),
///     Rule { action: "question".into(), resource: "*".into(), effect: Effect::Deny },
/// ]);
/// # Ok::<(), atelier_core::LimitsError>(())
/// ```
#[must_use]
pub fn rules(brief: &Brief) -> Vec<Rule> {
    let branch = &brief.branch;

    let mut rules: Vec<Rule> = ALLOWED
        .iter()
        .map(|action| rule(action, "*", Effect::Allow))
        .collect();

    rules.push(rule("shell", "git push *", Effect::Deny));

    for push in [
        "git push origin",
        "git push -u origin",
        "git push --set-upstream origin",
    ] {
        rules.push(rule("shell", &format!("{push} {branch}"), Effect::Allow));
    }

    rules.extend(
        brief
            .denied
            .iter()
            .map(|pattern| rule("shell", pattern, Effect::Deny)),
    );
    rules.push(rule("question", "*", Effect::Deny));

    rules
}
