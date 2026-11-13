//! The forge's kinds of step.

use std::path::Path;

use atelier_core::{
    Brief, Issue, IssueKey, Kind, Marks, MemoryTree, Needs, Observed, Outcome, Outcomes, Plugin,
    PullRequest, Registry, Repo, Rollup, RunContext, Setting, Settings, Started, Step, StepSpec,
};
use atelier_github::GitHub;

#[test]
fn the_forge_offers_checks_ready_and_merge_which_take_no_settings() {
    let github = GitHub::new();
    let tree = MemoryTree::default();
    let extra = Settings::default().with("rounds_label", Setting::Text("x".into()));
    let names: Vec<(&str, Outcomes, bool)> = github
        .kinds()
        .into_iter()
        .map(
            |Kind {
                 name,
                 outcomes,
                 resident,
             }| (name, outcomes, resident),
        )
        .collect();

    assert_eq!(
        names,
        [
            ("checks", Outcomes::Own(&["passed"]), false),
            ("ready", Outcomes::Own(&["passed"]), false),
            ("merge", Outcomes::Own(&["merged"]), false),
        ]
    );

    for kind in ["checks", "ready", "merge"] {
        assert_eq!(
            github.configure(kind, &Settings::default(), &tree).unwrap(),
            Settings::default()
        );
        assert_eq!(
            github
                .configure(kind, &extra, &tree)
                .unwrap_err()
                .to_string(),
            "rounds_label: unknown; it takes no settings"
        );
    }
}

#[test]
fn github_registers_its_kinds_as_the_forge() {
    let github = GitHub::new();
    let plugins: [&dyn Plugin; 1] = [&github];
    let registry = Registry::new(&plugins);
    let step = Settings::default()
        .with("id", Setting::Text("checks".into()))
        .with("uses", Setting::Text("forge.checks".into()));

    let pipeline = atelier_core::Pipeline::read(
        "build",
        &[step],
        &registry.kinds(None, "github", &["github"]),
        &MemoryTree::default(),
    )
    .unwrap();

    assert_eq!(pipeline.steps()[0].resolved(), "github.checks");
}
fn step(kind: &str) -> StepSpec {
    StepSpec {
        id: kind.into(),
        uses: format!("forge.{kind}"),
        plugin: "github",
        kind: kind.into(),
        needs: Needs::default(),
        outcomes: vec!["passed".into()],
        rounds: 1,
        restart: false,
        resident: false,
        settings: Settings::default(),
    }
}

#[test]
fn the_forge_s_steps_read_the_pull_request_on_the_run_s_branch() {
    let issue = Issue {
        key: IssueKey {
            tracker: "github".into(),
            id: "12".into(),
        },
        title: "Fix the dead link".into(),
        open: true,
        marks: Marks::default(),
    };
    let brief = Brief::new("#12", &issue, "issue-12", "main");
    let repo = Repo("StevenJPx2/atelier".into());
    let run = RunContext {
        repo: &repo,
        local: Path::new("/clones/atelier.git"),
        base: "main",
        issue: &issue,
        name: "issue-12",
        branch: "issue-12",
        worktree: None,
        folder: Path::new("/steps"),
        brief: &brief,
        session: None,
        pull_request: Some(PullRequest::Open {
            number: 8,
            draft: true,
            failed_rounds: 0,
            checks: Rollup::Passed,
        }),
        failed: 0,
        settled: false,
    };
    let github = GitHub::new();

    let told = github.start(&run, &step("ready")).unwrap();
    let checks = github.observe(&run, &step("checks")).unwrap();
    let ready = github.observe(&run, &step("ready")).unwrap();

    assert!(matches!(told, Started::Tell(text) if text.contains("Every check passed")));
    assert_eq!(
        checks,
        Observed::Settled(Outcome {
            name: "passed".into(),
            why: "every check passed".into(),
        })
    );
    assert_eq!(ready, Observed::Running);
}
