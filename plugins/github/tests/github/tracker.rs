use atelier_core::{Failure, Filter, Listing, Mark, Repo, Tracker};

use crate::stand_in::StandIn;

fn repo() -> Repo {
    Repo("StevenJPx2/atelier".into())
}

fn ids(listing: &Listing) -> Vec<&str> {
    match listing {
        Listing::Changed { issues, .. } => {
            issues.iter().map(|issue| issue.key.id.as_str()).collect()
        }
        Listing::Unchanged => vec![],
    }
}

#[test]
fn one_page_keeps_its_tag_and_an_unchanged_repository_answers_304() {
    let gh = StandIn::new(&[Some("issues-page-2.txt"), Some("issues-304.txt")]);
    let github = gh.github();

    let first = github
        .issues(&repo(), Mark::Build, &Filter::default(), None)
        .unwrap();
    let Listing::Changed {
        etag: Some(etag), ..
    } = &first
    else {
        panic!("{first:?}")
    };
    let second = github
        .issues(&repo(), Mark::Build, &Filter::default(), Some(etag))
        .unwrap();

    assert_eq!(ids(&first), ["3", "2", "1"]);
    assert_eq!(second, Listing::Unchanged);
    assert!(gh.calls()[1].contains(&format!("If-None-Match: {etag}")));
}

#[test]
fn pages_are_followed_pull_requests_dropped_and_no_tag_kept() {
    let gh = StandIn::new(&[Some("issues-with-pulls.txt"), Some("issues-page-2.txt")]);

    let listing = gh
        .github()
        .issues(&repo(), Mark::Build, &Filter::default(), None)
        .unwrap();

    assert_eq!(ids(&listing), ["7", "3", "2", "1"]);
    assert!(matches!(listing, Listing::Changed { etag: None, .. }));
    assert!(gh.calls()[1][2].contains("after="), "{:?}", gh.calls()[1]);
}

#[test]
fn a_filter_narrows_the_listing_by_mark_assignee_author_and_labels() {
    let gh = StandIn::new(&[Some("issues-page-2.txt")]);
    let filter = Filter {
        assignee: Some("StevenJPx2".into()),
        author: Some("someone else".into()),
        labels: vec!["backend".into()],
        query: None,
    };

    gh.github()
        .issues(&repo(), Mark::Working, &filter, None)
        .unwrap();

    assert_eq!(
        gh.calls()[0][2],
        "repos/StevenJPx2/atelier/issues?state=open&per_page=100\
         &labels=atelier%3Aworking%2Cbackend&assignee=StevenJPx2&creator=someone%20else"
    );
}

#[test]
fn a_query_goes_through_search_with_no_tag() {
    let gh = StandIn::new(&[Some("issues-page-2.txt")]);
    let filter = Filter {
        query: Some("milestone:v1".into()),
        ..Filter::default()
    };

    let error = gh
        .github()
        .issues(&repo(), Mark::Build, &filter, Some("\"tag\""))
        .unwrap_err();

    let call = &gh.calls()[0];
    assert!(
        call[2].starts_with("search/issues?per_page=100&q=repo%3AStevenJPx2%2Fatelier"),
        "{call:?}"
    );
    assert!(
        call[2].ends_with("label%3Aatelier%3Abuild%20milestone%3Av1"),
        "{call:?}"
    );
    assert!(!call.iter().any(|arg| arg.starts_with("If-None-Match")));
    assert!(
        error.message.contains("searching issues"),
        "a list where search items belong: {error}"
    );
}

#[test]
fn an_issue_is_read_and_a_missing_one_is_a_permanent_failure() {
    let gh = StandIn::new(&[Some("issue-6.txt"), Some("issue-404.txt")]);
    let github = gh.github();

    let issue = github.issue(&repo(), "6").unwrap();
    let missing = github.issue(&repo(), "999999").unwrap_err();

    assert_eq!(
        issue.title,
        "Plan: the github plugin, tracker and forge, slice 1"
    );
    assert!(issue.open);
    assert!(missing.is_permanent());
    assert!(missing.message.contains("404"), "{missing}");
}

#[test]
fn a_rate_limit_waits_and_an_unreachable_github_retries() {
    let gh = StandIn::new(&[Some("rate-limited.txt"), None]);
    let github = gh.github();

    let limited = github.issue(&repo(), "6").unwrap_err();
    let unreachable = github.issue(&repo(), "6").unwrap_err();

    assert_eq!(
        limited.failure,
        Failure::RateLimited {
            retry_after_secs: 60
        }
    );
    assert_eq!(unreachable.failure, Failure::Transient);
}

#[test]
fn marks_comments_and_closing_change_the_issue_and_a_missing_label_is_no_failure() {
    let gh = StandIn::new(&[
        Some("issue-6.txt"),
        Some("issue-404.txt"),
        Some("issue-6.txt"),
        Some("issue-6.txt"),
    ]);
    let github = gh.github();

    github.mark(&repo(), "12", Mark::Working).unwrap();
    github.unmark(&repo(), "12", Mark::Build).unwrap();
    github
        .comment(&repo(), "12", "atelier started a builder")
        .unwrap();
    github.close(&repo(), "12").unwrap();

    let calls = gh.calls();
    assert_eq!(
        calls[0][2..],
        [
            "--method",
            "POST",
            "repos/StevenJPx2/atelier/issues/12/labels",
            "--raw-field",
            "labels[]=atelier:working"
        ]
    );
    assert_eq!(
        calls[1][2..],
        [
            "--method",
            "DELETE",
            "repos/StevenJPx2/atelier/issues/12/labels/atelier%3Abuild"
        ]
    );
    assert_eq!(calls[2][6], "body=atelier started a builder");
    assert_eq!(
        calls[3][5..],
        [
            "--raw-field",
            "state=closed",
            "--raw-field",
            "state_reason=completed"
        ]
    );
}
