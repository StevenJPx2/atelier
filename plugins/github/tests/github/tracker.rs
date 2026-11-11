use atelier_core::{Failure, Filter, Listing, Mark, Repo, Tracker};
use atelier_core::{Setting, Settings};

use crate::stand_in::Reply::{Captured, Done, Unreachable};
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
    let gh = StandIn::new(&[Captured("issues-page-2.txt"), Captured("issues-304.txt")]);
    let github = gh.tracker();

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
    let gh = StandIn::new(&[
        Captured("issues-with-pulls.txt"),
        Captured("issues-page-2.txt"),
    ]);

    let listing = gh
        .tracker()
        .issues(&repo(), Mark::Build, &Filter::default(), None)
        .unwrap();

    assert_eq!(ids(&listing), ["7", "3", "2", "1"]);
    assert!(matches!(listing, Listing::Changed { etag: None, .. }));
    assert!(gh.calls()[1][2].contains("after="), "{:?}", gh.calls()[1]);
}

#[test]
fn a_filter_narrows_the_listing_by_mark_assignee_author_and_labels() {
    let gh = StandIn::new(&[Captured("issues-page-2.txt")]);
    let filter = Filter {
        assignee: Some("StevenJPx2".into()),
        author: Some("someone else".into()),
        labels: vec!["backend".into()],
        query: None,
    };

    gh.tracker()
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
    let gh = StandIn::new(&[Captured("issues-page-2.txt")]);
    let filter = Filter {
        query: Some("milestone:v1".into()),
        ..Filter::default()
    };

    let error = gh
        .tracker()
        .issues(&repo(), Mark::Build, &filter, Some("\"tag\""))
        .unwrap_err();

    let call = &gh.calls()[0];
    assert!(
        call[2].starts_with("search/issues?per_page=100&q=repo%3AStevenJPx2%2Fatelier"),
        "{call:?}"
    );
    assert!(
        call[2].ends_with("label%3A%22atelier%3Abuild%22%20milestone%3Av1"),
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
    let gh = StandIn::new(&[Captured("issue-6.txt"), Captured("issue-404.txt")]);
    let github = gh.tracker();

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
fn a_rate_limit_waits_a_refusal_is_final_and_an_unreachable_github_retries() {
    let gh = StandIn::new(&[
        Captured("rate-limited.txt"),
        Captured("forbidden.txt"),
        Unreachable,
    ]);
    let tracker = gh.tracker();

    let limited = tracker.issue(&repo(), "6").unwrap_err();
    let forbidden = tracker.mark(&repo(), "1", Mark::Build).unwrap_err();
    let unreachable = tracker.issue(&repo(), "6").unwrap_err();

    assert_eq!(
        limited.failure,
        Failure::RateLimited {
            retry_after_secs: 60
        }
    );
    assert_eq!(forbidden.failure, Failure::Permanent);
    assert_eq!(unreachable.failure, Failure::Transient);
}

#[test]
fn marks_comments_and_closing_change_the_issue_and_a_missing_label_is_no_failure() {
    let gh = StandIn::new(&[
        Captured("issue-6.txt"),
        Captured("issue-404.txt"),
        Captured("issue-6.txt"),
        Captured("issue-6.txt"),
    ]);
    let github = gh.tracker();

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

#[test]
fn labels_are_made_once_each_and_a_refusal_stops_them() {
    let gh = StandIn::new(&[Done, Done, Done, Done, Done, Unreachable]);
    let github = gh.tracker();

    github.create_labels(&repo()).unwrap();
    let error = github.create_labels(&repo()).unwrap_err();

    let calls = gh.calls();
    assert_eq!(calls.len(), 6);
    assert_eq!(
        calls[0][..5],
        ["label", "create", "atelier:build", "--force", "--repo"]
    );
    assert_eq!(calls[3][2], "improvement");
    assert!(
        error.message.contains("gh label create atelier:working"),
        "{error}"
    );
}

#[test]
fn a_repository_that_is_not_owner_and_name_is_refused_before_any_call() {
    let gh = StandIn::new(&[]);
    let github = gh.tracker();

    for name in ["atelier", "/atelier", "StevenJPx2/", "a/b/c"] {
        let error = github.issues(&Repo(name.into()), Mark::Build, &Filter::default(), None);

        assert!(error.unwrap_err().is_permanent(), "{name}");
    }

    assert_eq!(gh.calls().len(), 0);
}

#[test]
fn a_project_s_own_labels_are_listed_set_made_and_briefed() {
    let gh = StandIn::new(&[
        Captured("issues-page-2.txt"),
        Captured("issue-6.txt"),
        Captured("issue-6.txt"),
        Done,
        Done,
        Done,
        Done,
    ]);
    let working = Settings::default()
        .with("name", Setting::Text("bot:working".into()))
        .with("color", Setting::Text("5319e7".into()));
    let blocked = Settings::default().with("name", Setting::Text("bot:stuck".into()));
    let labels = Settings::default()
        .with("working", Setting::Table(working))
        .with("blocked", Setting::Table(blocked));
    let tracker = gh.tracker_with(&Settings::default().with("labels", Setting::Table(labels)));

    tracker
        .issues(&repo(), Mark::Working, &Filter::default(), None)
        .unwrap();
    tracker.mark(&repo(), "12", Mark::Blocked).unwrap();
    let issue = tracker.issue(&repo(), "6").unwrap();
    tracker.create_labels(&repo()).unwrap();

    let calls = gh.calls();
    assert!(
        calls[0][2].ends_with("&labels=bot%3Aworking"),
        "{:?}",
        calls[0]
    );
    assert_eq!(calls[1][6], "labels[]=bot:stuck");
    assert_eq!(
        calls[4][2..],
        [
            "bot:working",
            "--force",
            "--repo",
            "StevenJPx2/atelier",
            "--color",
            "5319e7",
            "--description",
            "An atelier builder holds this issue"
        ]
    );
    assert!(Tracker::brief(&tracker, &repo(), &issue).contains("--add-label bot:stuck"));
}
