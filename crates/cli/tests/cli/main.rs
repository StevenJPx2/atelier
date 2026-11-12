//! The real `atelier` binary, with real git and a real `origin`, and
//! stand-ins for GitHub and `OpenCode` built from their captured replies.

mod sandbox;

use std::fs;

use sandbox::{REPO, Sandbox, git};
use serde_json::json;

fn labelled(labels: &[&str]) -> serde_json::Value {
    json!({ "12": { "title": "Fix the dead link", "state": "open", "labels": labels, "assignee": null } })
}

fn sessions(sandbox: &Sandbox) -> usize {
    let created = |call: &&String| call.starts_with("api post /api/session --data");

    sandbox.opencode_calls().iter().filter(created).count()
}

fn listings(sandbox: &Sandbox) -> usize {
    sandbox
        .gh_calls()
        .iter()
        .filter(|call| call.get(2).is_some_and(|path| path.contains("/issues?")))
        .count()
}

#[test]
fn a_labelled_issue_gets_one_builder_across_polls_and_a_restart() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));

    let daemon = sandbox.daemon();
    sandbox.wait("a builder", |sandbox| {
        sandbox.phase("12").as_deref() == Some("building")
    });
    let before = listings(&sandbox);
    sandbox.wait("a second poll", |sandbox| listings(sandbox) >= before + 2);
    drop(daemon);

    let restarted = sandbox.daemon();
    let before = listings(&sandbox);
    sandbox.wait("a poll after the restart", |sandbox| {
        listings(sandbox) >= before + 2
    });
    drop(restarted);

    let issue = &sandbox.world()["issues"]["12"];
    let worktree = sandbox
        .home
        .join(".local/share/atelier/worktrees/StevenJPx2/demo/issue-12");
    assert_eq!(sessions(&sandbox), 1, "{:?}", sandbox.opencode_calls());
    assert_eq!(issue["labels"], json!(["atelier:working"]));
    assert_eq!(issue["comments"].as_array().map(Vec::len), Some(1));
    assert!(
        issue["comments"][0]
            .as_str()
            .unwrap()
            .contains("issue-12-fix-the-dead-link")
    );
    assert_eq!(
        git(&worktree, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "issue-12-fix-the-dead-link"
    );

    let log = sandbox.log();
    let first = log.lines().next().unwrap_or_default();
    assert!(
        first.starts_with("20") && first.contains("Z atelier daemon"),
        "{first}"
    );
    assert!(
        log.contains(" StevenJPx2/demo github#12 Untracked → Claiming"),
        "{log}"
    );
    assert!(
        log.contains(" StevenJPx2/demo github#12 Starting → Building"),
        "{log}"
    );

    let status = sandbox.status();
    assert!(status.contains(REPO), "{status}");
    assert!(
        status.contains("base        main                     found"),
        "{status}"
    );
    assert!(status.contains("github#12   Building"), "{status}");
}

#[test]
fn a_second_daemon_refuses_to_start() {
    let sandbox = Sandbox::new(&labelled(&[]));
    let first = sandbox.daemon();
    sandbox.wait("the first daemon", |sandbox| {
        sandbox.log().contains("watching")
    });

    let second = sandbox.atelier(&["daemon"]);

    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(!second.status.success());
    assert!(
        stderr.contains(&format!("already running (process {})", first.0.id())),
        "{stderr}"
    );
}

#[test]
fn an_issue_assigned_to_someone_else_is_left_alone_under_your_filter() {
    let sandbox = Sandbox::new(&json!({
        "12": { "title": "Not mine", "state": "open", "labels": ["atelier:build"], "assignee": "someone-else" }
    }));
    let projects = sandbox.home.join(".config/atelier/projects.toml");
    let mine = fs::read_to_string(&projects).unwrap() + "filter.github = { assignee = \"me\" }\n";
    fs::write(&projects, mine).unwrap();

    let daemon = sandbox.daemon();
    sandbox.wait("two listings", |sandbox| listings(sandbox) >= 2);
    drop(daemon);

    let asked = sandbox.gh_calls();
    assert!(
        asked
            .iter()
            .any(|call| call[2].ends_with("labels=atelier%3Abuild&assignee=me")),
        "{asked:?}"
    );
    assert_eq!(sandbox.phase("12"), None);
    assert_eq!(
        sandbox.world()["issues"]["12"]["labels"],
        json!(["atelier:build"])
    );
    assert_eq!(sessions(&sandbox), 0);
}

#[test]
fn a_builder_that_cannot_start_blocks_its_issue() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    fs::write(sandbox.bin.join("refuse"), "").unwrap();

    let daemon = sandbox.daemon();
    sandbox.wait("a block", |sandbox| {
        sandbox.phase("12").as_deref() == Some("blocked")
    });
    drop(daemon);

    let issue = &sandbox.world()["issues"]["12"];
    assert_eq!(issue["labels"], json!(["atelier:blocked"]));
    assert!(
        issue["comments"][0]
            .as_str()
            .unwrap()
            .contains("could not start")
    );
}

#[test]
fn a_plugin_the_build_lacks_fails_that_project_by_name() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    sandbox.config("forge = \"gitlab\"\n");

    let daemon = sandbox.daemon();
    sandbox.wait("the refusal", |sandbox| {
        sandbox
            .log()
            .contains("no forge plugin \"gitlab\"; it has github")
    });
    drop(daemon);

    assert_eq!(sandbox.phase("12"), None);
}

#[test]
fn init_makes_the_labels_the_project_sets() {
    let sandbox = Sandbox::new(&labelled(&[]));
    sandbox.config("[[tracker]]\nlabels.working = { name = \"bot:working\" }\n");

    let init = sandbox.atelier(&["init", "--path", &sandbox.checkout.to_string_lossy()]);

    let stdout = String::from_utf8(init.stdout).unwrap();
    let made: Vec<String> = sandbox
        .gh_calls()
        .iter()
        .map(|call| call[2].clone())
        .collect();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    assert_eq!(
        made,
        [
            "atelier:build",
            "bot:working",
            "atelier:blocked",
            "improvement"
        ]
    );
    assert!(
        stdout.contains("labels atelier:build, bot:working"),
        "{stdout}"
    );
}

#[test]
fn add_refuses_another_repository_and_a_second_add() {
    let sandbox = Sandbox::new(&labelled(&[]));
    let path = sandbox.checkout.to_string_lossy().into_owned();

    let other = sandbox.atelier(&["add", "StevenJPx2/other", "--path", &path]);
    let again = sandbox.atelier(&["add", REPO, "--path", &path]);

    assert!(
        String::from_utf8_lossy(&other.stderr)
            .contains("has origin StevenJPx2/demo, not StevenJPx2/other")
    );
    assert!(String::from_utf8_lossy(&again.stderr).contains("already watched"));
}

#[test]
fn the_default_branch_names_the_base_and_the_base_holds_the_rest() {
    let sandbox = Sandbox::new(&labelled(&[]));
    sandbox.config("base = \"v2\"\n[limits]\nred_rounds = 9\n");
    sandbox.config_on("v2", "poll = 20\n[limits]\nred_rounds = 3\n");

    let status = sandbox.status();

    for line in [
        "base        v2                       set",
        "red_rounds  3                        set",
        "poll        20 s                     set",
        "forge       github                   found",
    ] {
        assert!(status.contains(line), "{line}\n{status}");
    }
}

#[test]
fn a_base_that_is_the_default_branch_reads_the_rest_there() {
    let sandbox = Sandbox::new(&labelled(&[]));
    sandbox.config("base = \"main\"\npoll = 15\n[limits]\nred_rounds = 2\n");

    let status = sandbox.status();

    assert!(
        status.contains("base        main                     set"),
        "{status}"
    );
    assert!(
        status.contains("red_rounds  2                        set"),
        "{status}"
    );
    assert!(
        status.contains("poll        15 s                     set"),
        "{status}"
    );
}

#[test]
fn settings_atelier_cannot_run_are_refused_by_name() {
    let cases = [
        ("poll = 10\n", "poll is 10; it must be at least 15 seconds"),
        (
            "checks = [\"docs\"]\n",
            "this atelier has no checks plugin \"docs\"; it has none",
        ),
        (
            "stages = [\"critic\"]\n",
            "this atelier has no stage plugin \"critic\"; it has none",
        ),
        (
            "[limits]\ncritic_rounds = 2\n",
            "limits.critic_rounds is unknown; it takes red_rounds",
        ),
    ];

    for (config, refusal) in cases {
        let sandbox = Sandbox::new(&labelled(&[]));
        sandbox.config(config);

        let status = sandbox.status();

        assert!(status.contains(refusal), "{config}\n{status}");
    }
}

#[test]
fn two_trackers_from_one_plugin_are_refused() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    sandbox.config("[[tracker]]\nplugin = \"github\"\n\n[[tracker]]\nplugin = \"github\"\n");

    let daemon = sandbox.daemon();
    sandbox.wait("the refusal", |sandbox| {
        sandbox.log().contains("two trackers use the github plugin")
    });
    drop(daemon);

    assert_eq!(sandbox.phase("12"), None);
}

#[test]
fn a_project_removed_from_your_list_stops_being_watched() {
    let sandbox = Sandbox::new(&labelled(&[]));
    let daemon = sandbox.daemon();
    sandbox.wait("watching", |sandbox| {
        sandbox.log().contains("StevenJPx2/demo watching")
    });

    fs::write(sandbox.home.join(".config/atelier/projects.toml"), "").unwrap();
    sandbox.wait("the stop", |sandbox| {
        sandbox.log().contains("StevenJPx2/demo stopped watching")
    });
    drop(daemon);
}

#[test]
fn xdg_folders_are_used_and_an_empty_one_is_ignored() {
    let sandbox = Sandbox::new(&labelled(&[]));
    let elsewhere = sandbox.root.join("xdg");
    let path = sandbox.checkout.to_string_lossy().into_owned();

    let added = sandbox
        .command(&["add", REPO, "--path", &path])
        .env("XDG_CONFIG_HOME", &elsewhere)
        .output()
        .unwrap();
    let status = sandbox
        .command(&["status"])
        .env("XDG_CONFIG_HOME", "")
        .output()
        .unwrap();

    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    assert!(elsewhere.join("atelier/projects.toml").exists());
    assert!(String::from_utf8_lossy(&status.stdout).contains(REPO));
}
