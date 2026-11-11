//! Once against the real GitHub, read-only, when `gh` is signed in.

use std::process::Command;

use atelier_core::{Filter, Forge, Listing, Mark, PullRequest, Repo, Settings, Tracker};
use atelier_github::GitHub;

fn signed_in() -> bool {
    Command::new("gh")
        .args(["auth", "status"])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn the_real_github_answers_as_the_captured_replies_do() {
    if !signed_in() {
        eprintln!("skipped: gh is not signed in");
        return;
    }

    let github = GitHub::new();
    let repo = Repo("StevenJPx2/atelier".into());
    let filter = Filter {
        labels: vec!["plan".into()],
        ..Filter::default()
    };

    let tracker = github.tracker(&Settings::default()).unwrap();
    let issue = tracker.issue(&repo, "6").unwrap();
    let merged = github.pull_request(&repo, "feat/slice-0-setup").unwrap();
    let first = tracker.issues(&repo, Mark::Build, &filter, None).unwrap();
    let Listing::Changed {
        etag: Some(etag), ..
    } = first
    else {
        panic!("{first:?}")
    };
    let again = tracker
        .issues(&repo, Mark::Build, &filter, Some(&etag))
        .unwrap();

    assert_eq!(
        issue.title,
        "Plan: the github plugin, tracker and forge, slice 1"
    );
    assert_eq!(merged, Some(PullRequest::Merged { number: 8 }));
    assert_eq!(again, Listing::Unchanged);
}
