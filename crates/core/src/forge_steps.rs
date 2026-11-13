//! The kinds of step every forge offers, read from the pull request it
//! reports, and how the builder's step settles from it. A forge plugin
//! offers them as its own, so `forge.checks` means the same on any forge.

use crate::{
    Kind, Observed, Outcome, Outcomes, PluginError, PullRequest, Rollup, RunContext, Started,
};

/// What the resident steps hear when every check passed; nothing else
/// tells the builder of passing checks.
const GREEN: &str = "Every check passed on your pull request. Mark it ready, comment on the \
                     issue, and keep answering reviews until the merge.";

const PASSED: Outcomes = Outcomes::Own(&["passed"]);

/// The forge's kinds: `checks`, `ready` and `merge`.
///
/// `checks` passes when every check on the pull request's head passed and
/// fails a round for each head commit with a failed check; `ready` tells
/// the builder, and passes once the pull request is marked ready; `merge`
/// settles `merged` once a person merges it. A pull request closed
/// without merging fails each of them for good.
#[must_use]
pub fn forge_kinds() -> Vec<Kind> {
    let kind = |name, outcomes| Kind {
        name,
        outcomes,
        resident: false,
    };

    vec![
        kind("checks", PASSED),
        kind("ready", PASSED),
        kind("merge", Outcomes::Own(&["merged"])),
    ]
}

/// Starts a forge step: `ready` tells the resident steps every check
/// passed, unless the pull request merged already; the others wait on CI
/// and on a person.
///
/// # Errors
///
/// A kind no forge offers.
pub fn start_forge_step(kind: &str, run: &RunContext<'_>) -> Result<Started, PluginError> {
    let merged = matches!(run.pull_request, Some(PullRequest::Merged { .. }));

    match kind {
        "checks" | "merge" => Ok(Started::Nothing),
        "ready" if merged => Ok(Started::Nothing),
        "ready" => Ok(Started::Tell(GREEN.to_owned())),
        _ => Err(unknown(kind)),
    }
}

/// What the pull request on the run's branch says of a forge step.
///
/// ```
/// use atelier_core::{Observed, PullRequest, Rollup};
/// # use atelier_core::{Brief, Issue, IssueKey, Marks, Repo, RunContext, observe_forge_step};
/// # use std::path::Path;
/// # let issue = Issue { key: IssueKey { tracker: "github".into(), id: "12".into() },
/// #     title: "Fix".into(), open: true, marks: Marks::default() };
/// # let brief = Brief::new("#12", &issue, "issue-12", "main");
/// # let repo = Repo("o/r".into());
/// let pull_request = PullRequest::Open { number: 8, draft: true, failed_rounds: 2, checks: Rollup::Failed };
/// # let run = RunContext { repo: &repo, local: Path::new("/"), base: "main", issue: &issue,
/// #     name: "issue-12", branch: "issue-12", worktree: None, folder: Path::new("/"),
/// #     brief: &brief, session: None, pull_request: Some(pull_request), failed: 1, settled: false };
///
/// // one failed round counted so far, and a second head commit failed
/// let seen = observe_forge_step("checks", &run)?;
///
/// assert_eq!(seen, Observed::Failed { why: "CI failed on 2 commits".into(), last: false });
/// # Ok::<(), atelier_core::PluginError>(())
/// ```
///
/// # Errors
///
/// A kind no forge offers.
pub fn observe_forge_step(kind: &str, run: &RunContext<'_>) -> Result<Observed, PluginError> {
    let seen = match (kind, run.pull_request) {
        ("checks" | "ready" | "merge", Some(PullRequest::Closed { .. })) => closed(),
        ("merge", Some(PullRequest::Merged { number })) => {
            settled("merged", format!("#{number} merged"))
        }
        ("checks" | "ready", Some(PullRequest::Merged { number })) => {
            settled("passed", format!("#{number} merged"))
        }
        (
            "checks",
            Some(PullRequest::Open {
                failed_rounds,
                checks,
                ..
            }),
        ) => self::checks(run, failed_rounds, checks),
        ("ready", Some(PullRequest::Open { draft: false, .. })) => {
            settled("passed", "marked ready".to_owned())
        }
        ("checks" | "ready" | "merge", None)
        | ("ready" | "merge", Some(PullRequest::Open { .. })) => Observed::Running,
        _ => return Err(unknown(kind)),
    };

    Ok(seen)
}

/// The builder's step settles `opened` once a pull request is on its
/// branch, and fails for good when it closed without merging.
#[must_use]
pub fn observe_builder(run: &RunContext<'_>) -> Observed {
    match run.pull_request {
        Some(PullRequest::Open { number, .. } | PullRequest::Merged { number }) => {
            settled("opened", format!("opened #{number}"))
        }
        Some(PullRequest::Closed { .. }) => closed(),
        None => Observed::Running,
    }
}

/// A running `checks` fails a round for each head commit with a failed
/// check, and passes when every check passed; a settled one runs again
/// when a push makes its checks pending or fail.
fn checks(run: &RunContext<'_>, failed_rounds: u8, checks: Rollup) -> Observed {
    let failed = failed_rounds > run.failed;

    match (run.settled, failed, checks) {
        (true, true, _) | (true, false, Rollup::Pending) => Observed::Restarted,
        (false, true, _) => Observed::Failed {
            why: format!("CI failed on {failed_rounds} commits"),
            last: false,
        },
        (_, false, Rollup::Passed) => settled("passed", "every check passed".to_owned()),
        _ => Observed::Running,
    }
}

fn settled(name: &str, why: String) -> Observed {
    Observed::Settled(Outcome {
        name: name.to_owned(),
        why,
    })
}

fn closed() -> Observed {
    Observed::Failed {
        why: "its pull request was closed without merging".to_owned(),
        last: true,
    }
}

fn unknown(kind: &str) -> PluginError {
    PluginError::permanent(format!("no forge offers the step kind {kind}"))
}
#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::{Brief, Issue, IssueKey, Marks, Repo};

    fn issue() -> Issue {
        Issue {
            key: IssueKey {
                tracker: "github".into(),
                id: "12".into(),
            },
            title: "Fix the dead link".into(),
            open: true,
            marks: Marks::default(),
        }
    }

    /// Calls `with` on a run that sees `pull_request`, after `failed`
    /// rounds, settled or not.
    fn with_run<T>(
        pull_request: Option<PullRequest>,
        failed: u8,
        settled: bool,
        with: impl FnOnce(&RunContext<'_>) -> T,
    ) -> T {
        let issue = issue();
        let brief = Brief::new("#12", &issue, "issue-12", "main");
        let repo = Repo("o/r".into());
        let run = RunContext {
            repo: &repo,
            local: Path::new("/local"),
            base: "main",
            issue: &issue,
            name: "issue-12",
            branch: "issue-12",
            worktree: None,
            folder: Path::new("/steps"),
            brief: &brief,
            session: None,
            pull_request,
            failed,
            settled,
        };

        with(&run)
    }

    fn seen(kind: &str, pull_request: Option<PullRequest>, failed: u8, settled: bool) -> Observed {
        with_run(pull_request, failed, settled, |run| {
            if kind == "builder" {
                observe_builder(run)
            } else {
                observe_forge_step(kind, run).unwrap()
            }
        })
    }

    fn started(kind: &str, pull_request: Option<PullRequest>) -> Result<Started, PluginError> {
        with_run(pull_request, 0, false, |run| start_forge_step(kind, run))
    }

    const fn pull(failed_rounds: u8, checks: Rollup, draft: bool) -> PullRequest {
        PullRequest::Open {
            number: 8,
            draft,
            failed_rounds,
            checks,
        }
    }

    const MERGED: Option<PullRequest> = Some(PullRequest::Merged { number: 8 });
    const CLOSED: Option<PullRequest> = Some(PullRequest::Closed { number: 8 });

    fn passed(why: &str) -> Observed {
        settled("passed", why.to_owned())
    }

    #[test]
    fn the_forge_offers_checks_ready_and_merge() {
        let names: Vec<&str> = forge_kinds().iter().map(|kind| kind.name).collect();

        assert_eq!(names, ["checks", "ready", "merge"]);
        assert_eq!(forge_kinds()[2].outcomes, Outcomes::Own(&["merged"]));
        assert!(forge_kinds().iter().all(|kind| !kind.resident));
    }

    #[test]
    fn ready_tells_the_builder_unless_the_pull_request_merged() {
        let Ok(Started::Tell(green)) = started("ready", Some(pull(0, Rollup::Passed, true))) else {
            panic!("ready told nothing");
        };

        assert!(green.contains("Every check passed"));
        assert_eq!(started("ready", MERGED), Ok(Started::Nothing));
        assert_eq!(started("checks", None), Ok(Started::Nothing));
        assert_eq!(started("merge", None), Ok(Started::Nothing));
        assert!(started("reviews", None).unwrap_err().is_permanent());
    }

    #[test]
    fn checks_fail_a_round_per_failed_head_and_pass_when_every_check_passed() {
        let failing = Observed::Failed {
            why: "CI failed on 2 commits".into(),
            last: false,
        };

        let cases = [
            (None, 0, Observed::Running),
            (Some(pull(0, Rollup::Pending, true)), 0, Observed::Running),
            (Some(pull(1, Rollup::Failed, true)), 1, Observed::Running),
            (Some(pull(2, Rollup::Failed, true)), 1, failing),
            (
                Some(pull(1, Rollup::Passed, true)),
                1,
                passed("every check passed"),
            ),
            (MERGED, 0, passed("#8 merged")),
            (CLOSED, 0, closed()),
        ];

        for (pull_request, failed, expected) in cases {
            assert_eq!(
                seen("checks", pull_request, failed, false),
                expected,
                "{pull_request:?}"
            );
        }
    }

    #[test]
    fn settled_checks_run_again_after_a_push() {
        let cases = [
            (
                Some(pull(1, Rollup::Pending, false)),
                1,
                Observed::Restarted,
            ),
            (Some(pull(2, Rollup::Failed, false)), 1, Observed::Restarted),
            (Some(pull(1, Rollup::Failed, false)), 1, Observed::Running),
            (
                Some(pull(1, Rollup::Passed, false)),
                1,
                passed("every check passed"),
            ),
        ];

        for (pull_request, failed, expected) in cases {
            assert_eq!(
                seen("checks", pull_request, failed, true),
                expected,
                "{pull_request:?}"
            );
        }
    }

    #[test]
    fn ready_passes_once_marked_ready_and_merge_once_merged() {
        let cases = [
            (
                "ready",
                Some(pull(0, Rollup::Passed, true)),
                Observed::Running,
            ),
            (
                "ready",
                Some(pull(0, Rollup::Passed, false)),
                passed("marked ready"),
            ),
            ("ready", MERGED, passed("#8 merged")),
            ("ready", CLOSED, closed()),
            (
                "merge",
                Some(pull(0, Rollup::Passed, false)),
                Observed::Running,
            ),
            ("merge", MERGED, settled("merged", "#8 merged".into())),
            ("merge", CLOSED, closed()),
            ("merge", None, Observed::Running),
        ];

        for (kind, pull_request, expected) in cases {
            assert_eq!(
                seen(kind, pull_request, 0, false),
                expected,
                "{kind} {pull_request:?}"
            );
        }
    }

    #[test]
    fn the_builder_opens_with_a_pull_request_on_its_branch() {
        let opened = settled("opened", "opened #8".into());

        assert_eq!(seen("builder", None, 0, false), Observed::Running);
        assert_eq!(
            seen("builder", Some(pull(0, Rollup::Pending, true)), 0, false),
            opened
        );
        assert_eq!(seen("builder", MERGED, 0, false), opened);
        assert_eq!(seen("builder", CLOSED, 0, false), closed());
        assert_eq!(
            closed(),
            Observed::Failed {
                why: "its pull request was closed without merging".into(),
                last: true
            }
        );
    }

    #[test]
    fn a_kind_no_forge_offers_is_refused() {
        let issue = issue();
        let brief = Brief::new("#12", &issue, "issue-12", "main");
        let repo = Repo("o/r".into());
        let run = RunContext {
            repo: &repo,
            local: Path::new("/local"),
            base: "main",
            issue: &issue,
            name: "issue-12",
            branch: "issue-12",
            worktree: None,
            folder: Path::new("/steps"),
            brief: &brief,
            session: None,
            pull_request: None,
            failed: 0,
            settled: false,
        };

        let refused = observe_forge_step("reviews", &run).unwrap_err();

        assert_eq!(refused.message, "no forge offers the step kind reviews");
    }
}
