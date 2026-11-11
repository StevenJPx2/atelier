use atelier_core::{Checks, Forge, Plugin, PullRequest, Registry, Repo};

use crate::stand_in::StandIn;

fn repo() -> Repo {
    Repo("StevenJPx2/atelier".into())
}

#[test]
fn a_pull_request_reads_its_state_drafts_and_failed_rounds() {
    let gh = StandIn::new(&[
        Some("pull-merged.txt"),
        Some("pull-open-failed-once.txt"),
        Some("pull-open.txt"),
        Some("pull-none.txt"),
    ]);
    let github = gh.github();

    let found: Vec<Option<PullRequest>> = [
        "feat/slice-0-setup",
        "feat/slice-0-setup",
        "feat/git-worktrees",
        "none",
    ]
    .into_iter()
    .map(|branch| github.pull_request(&repo(), branch).unwrap())
    .collect();

    assert_eq!(
        found,
        [
            Some(PullRequest::Merged { number: 8 }),
            Some(PullRequest::Open {
                number: 8,
                draft: true,
                failed_rounds: 1,
                checks: Checks::Passed
            }),
            Some(PullRequest::Open {
                number: 12,
                draft: false,
                failed_rounds: 0,
                checks: Checks::Passed
            }),
            None,
        ]
    );
    assert!(gh.calls()[2].contains(&"branch=feat/git-worktrees".to_owned()));
    assert!(gh.calls()[2].contains(&"owner=StevenJPx2".to_owned()));
}

#[test]
fn the_builder_may_not_merge_or_release() {
    let github = atelier_github::GitHub::new();

    assert_eq!(github.denied(), ["gh pr merge *", "gh release *"]);
    assert!(
        Forge::brief(&github, &repo(), "v2", "issue-12-x").contains("--base v2 --head issue-12-x")
    );
}

/// Core finds an issue's tracker by the name in its key, so the plugin must
/// register under the name its issues carry.
#[test]
fn the_plugin_registers_both_roles_under_the_name_its_issues_carry() {
    let gh = StandIn::new(&[Some("issue-6.txt")]);
    let github = gh.github();
    let registry = Registry::new(&[&github]);

    let issue = registry
        .find_tracker(github.name())
        .unwrap()
        .issue(&repo(), "6")
        .unwrap();

    assert_eq!(issue.key.tracker, github.name());
    assert!(registry.find_forge(github.name()).is_ok());
    assert!(registry.find_tracker("").is_err());
}
