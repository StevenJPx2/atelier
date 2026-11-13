//! A project's pipelines, read from its config on `origin` and checked
//! against the kinds of step this atelier's plugins offer.

use crate::labelled;
use crate::sandbox::Sandbox;

const SECURITY_AUDIT: &str = r#"
[[pipeline.security-audit.step]]
id = "worktree"
uses = "git.worktree"

[[pipeline.security-audit.step]]
id = "deps"
uses = "atelier.command"
run = "cargo deny check advisories"
after = ["worktree"]

[[pipeline.security-audit.step]]
id = "audit"
uses = "agent.session"
prompt = ".atelier/prompts/security-audit.md"
outcomes = ["found", "clean"]
after = ["worktree"]

[[pipeline.security-audit.step]]
id = "fix"
uses = "agent.session"
prompt = ".atelier/prompts/security-audit.md"
outcomes = ["opened"]
after_any = { audit = "found", deps = "failed" }
"#;

/// The lines of `status` from `first` on, as many as `count`.
fn block(status: &str, first: &str, count: usize) -> Vec<String> {
    status
        .lines()
        .skip_while(|line| !line.contains(first))
        .take(count)
        .map(|line| line.trim_end().to_owned())
        .collect()
}

#[test]
fn status_shows_the_built_in_build_and_each_written_pipeline_with_each_kind_resolved() {
    let sandbox = Sandbox::new(&labelled(&[]));
    sandbox.commit(
        ".atelier/prompts/security-audit.md",
        "Audit the dependencies.\n",
    );
    sandbox.config(SECURITY_AUDIT);

    let status = sandbox.status();

    assert_eq!(
        block(&status, "pipeline    build", 6),
        [
            "  pipeline    build                    found",
            "    worktree  git.worktree",
            "    builder   agent.session → opencode.session",
            "    checks    forge.checks → github.checks",
            "    ready     forge.ready → github.ready",
            "    merge     forge.merge → github.merge",
        ],
        "{status}"
    );
    assert_eq!(
        block(&status, "pipeline    security-audit", 5),
        [
            "  pipeline    security-audit           set",
            "    worktree  git.worktree",
            "    deps      atelier.command",
            "    audit     agent.session → opencode.session",
            "    fix       agent.session → opencode.session",
        ],
        "{status}"
    );
}

#[test]
fn a_written_build_replaces_the_built_in_one() {
    let sandbox = Sandbox::new(&labelled(&[]));
    sandbox.config("[[pipeline.build.step]]\nid = \"worktree\"\nuses = \"git.worktree\"\n");

    let status = sandbox.status();

    assert_eq!(
        block(&status, "pipeline    build", 3),
        [
            "  pipeline    build                    set",
            "    worktree  git.worktree",
        ],
        "{status}"
    );
}

#[test]
fn a_pipeline_atelier_cannot_run_refuses_the_project_with_its_name_and_why() {
    let cases = [
        (
            SECURITY_AUDIT.replace("audit = \"found\"", "audit = \"fond\""),
            ".atelier/config.toml: pipeline security-audit: fix: after_any names audit's fond; audit settles found, clean, or failed",
        ),
        (
            SECURITY_AUDIT.to_owned(),
            ".atelier/config.toml: pipeline security-audit: audit.prompt: .atelier/prompts/security-audit.md is not on the base branch",
        ),
        (
            "[[pipeline.review.step]]\nid = \"reviews\"\nuses = \"forge.reviews\"\n".to_owned(),
            ".atelier/config.toml: pipeline review: no step kind \"forge.reviews\"; github has checks, ready, merge",
        ),
        (
            "[pipeline.review]\nsteps = []\n".to_owned(),
            "unknown field `steps`, expected `step`",
        ),
    ];

    for (index, (config, refusal)) in cases.into_iter().enumerate() {
        let sandbox = Sandbox::new(&labelled(&[]));

        if index == 0 {
            sandbox.commit(".atelier/prompts/security-audit.md", "Audit it.\n");
        }

        sandbox.config(&config);
        let status = sandbox.status();

        assert!(status.contains(refusal), "{config}\n{status}");
    }
}

#[test]
fn the_daemon_takes_no_issue_in_a_project_whose_pipeline_it_cannot_run() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    sandbox.config("[[pipeline.review.step]]\nid = \"reviews\"\nuses = \"forge.reviews\"\n");

    let daemon = sandbox.daemon();
    sandbox.wait("the refusal", |sandbox| {
        sandbox.log().contains("no step kind \"forge.reviews\"")
    });
    drop(daemon);

    assert_eq!(sandbox.phase("12"), None);
}
