//! The real `atelier` binary, with real git and a real `origin`, and
//! stand-ins for GitHub and `OpenCode` built from their captured replies.

mod gates;
mod pipelines;
mod recovery;
mod sandbox;

use std::fs;

use sandbox::{REPO, Sandbox, git};
use serde_json::json;

pub(crate) fn labelled(labels: &[&str]) -> serde_json::Value {
    json!({ "12": { "title": "Fix the dead link", "state": "open", "labels": labels, "assignee": null } })
}

fn sessions(sandbox: &Sandbox) -> usize {
    let created = |call: &&String| call.starts_with("api post /api/session --data");

    sandbox.opencode_calls().iter().filter(created).count()
}

fn prompts(sandbox: &Sandbox) -> Vec<String> {
    sandbox
        .opencode_calls()
        .into_iter()
        .filter(|call| call.contains("/prompt "))
        .collect()
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
    assert!(
        status
            .lines()
            .any(|line| line.contains("github#12") && line.ends_with("  -")),
        "{status}"
    );
}

#[test]
fn passing_checks_wake_the_builder_to_mark_its_pull_request_ready() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    let daemon = sandbox.daemon();
    sandbox.wait("a builder", |sandbox| {
        sandbox.phase("12").as_deref() == Some("building")
    });

    sandbox.open_pull_request("issue-12-fix-the-dead-link", 8, "SUCCESS");
    sandbox.wait("the builder told", |sandbox| prompts(sandbox).len() >= 2);
    let before = listings(&sandbox);
    sandbox.wait("a later poll", |sandbox| listings(sandbox) >= before + 2);
    drop(daemon);

    let prompts = prompts(&sandbox);
    assert_eq!(sandbox.phase("12").as_deref(), Some("green"));
    assert_eq!(prompts.len(), 2, "{prompts:?}");
    assert!(
        prompts[1].starts_with("api post /api/session/ses_e34e74974ffezeihakyxaDRnbK/prompt "),
        "{prompts:?}"
    );
    assert!(prompts[1].contains("Every check passed"), "{prompts:?}");
}

#[test]
fn removing_the_working_label_interrupts_the_builder_and_tells_it_to_stand_down() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    let daemon = sandbox.daemon();
    sandbox.wait("a builder", |sandbox| {
        sandbox.phase("12").as_deref() == Some("building")
    });

    sandbox.set_labels("12", &[]);
    sandbox.wait("the builder stopped", |sandbox| {
        sandbox.phase("12").as_deref() == Some("stopped")
    });
    drop(daemon);

    let calls = sandbox.opencode_calls();
    let interrupt = calls.iter().position(|call| call.contains("/interrupt"));
    let told = calls.iter().rposition(|call| call.contains("/prompt "));
    let (Some(interrupt), Some(told)) = (interrupt, told) else {
        panic!("no interrupt, or no prompt: {calls:?}");
    };
    assert!(interrupt < told, "told before the interrupt: {calls:?}");
    assert!(calls[told].contains("Take no further action"), "{calls:?}");
}

#[test]
fn status_shows_the_pull_request_a_builder_opened() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    let daemon = sandbox.daemon();
    sandbox.wait("a builder", |sandbox| {
        sandbox.phase("12").as_deref() == Some("building")
    });

    sandbox.open_pull_request("issue-12-fix-the-dead-link", 8, "PENDING");
    sandbox.wait("the pull request", |sandbox| {
        sandbox.phase("12").as_deref() == Some("open")
    });
    drop(daemon);

    let before = sandbox.gh_calls().len();
    let status = sandbox.status();
    let line = status
        .lines()
        .find(|line| line.contains("github#12"))
        .unwrap_or_default();
    assert!(line.contains("Open { rounds: 0 }"), "{status}");
    assert!(line.ends_with("  #8"), "{status}");
    assert_eq!(sandbox.gh_calls().len(), before, "status asked GitHub");
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
    let stderr = String::from_utf8(init.stderr.clone()).unwrap();
    let made: Vec<String> = sandbox
        .gh_calls()
        .iter()
        .filter(|call| call[..2] == ["label", "create"])
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
    assert!(
        stderr.contains("no release build, so it writes no CI"),
        "{stderr}"
    );
    assert!(!sandbox.checkout.join(".github").exists());
}

#[test]
fn add_clones_the_project_and_refuses_one_it_cannot_reach_or_already_watches() {
    let sandbox = Sandbox::new(&labelled(&[]));

    let missing = sandbox.atelier(&["add", "StevenJPx2/missing"]);
    let unsafe_name = sandbox.atelier(&["add", "StevenJPx2/.."]);
    let again = sandbox.atelier(&["add", REPO]);

    let clone = sandbox.clone();
    assert_eq!(git(&clone, &["rev-parse", "--is-bare-repository"]), "true");
    assert_eq!(
        git(&clone, &["config", "--get", "remote.origin.url"]),
        "https://github.com/StevenJPx2/demo.git"
    );
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("cannot reach StevenJPx2/missing"),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
    assert!(
        !sandbox
            .home
            .join(".local/share/atelier/clones/StevenJPx2/missing.git")
            .exists()
    );
    assert!(String::from_utf8_lossy(&unsafe_name.stderr).contains("cannot name a folder"));
    assert!(String::from_utf8_lossy(&again.stderr).contains("already watched"));
    let projects = fs::read_to_string(sandbox.home.join(".config/atelier/projects.toml")).unwrap();
    assert_eq!(projects.matches("[[project]]").count(), 1, "{projects}");
}

#[test]
fn the_daemon_works_from_its_own_clone_and_never_from_your_checkout() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    let checkout = sandbox.checkout.to_string_lossy().into_owned();
    fs::write(
        sandbox.home.join(".config/atelier/projects.toml"),
        format!("[[project]]\nrepo = \"{REPO}\"\npath = \"{checkout}\"\n"),
    )
    .unwrap();
    fs::remove_dir_all(sandbox.clone()).unwrap();
    let before = git(&sandbox.checkout, &["for-each-ref", "--format=%(refname)"]);

    let daemon = sandbox.daemon();
    sandbox.wait("a builder", |sandbox| {
        sandbox.phase("12").as_deref() == Some("building")
    });
    drop(daemon);

    let worktree = sandbox
        .home
        .join(".local/share/atelier/worktrees/StevenJPx2/demo/issue-12");
    let common = git(
        &worktree,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    );
    assert_eq!(
        std::path::Path::new(&common),
        sandbox.clone().canonicalize().unwrap()
    );
    assert_eq!(
        git(&sandbox.checkout, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
    assert_eq!(
        git(&sandbox.checkout, &["for-each-ref", "--format=%(refname)"]),
        before
    );
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
            "this atelier has no checks plugin \"docs\"; it has commands",
        ),
        (
            "[commandz.fmt]\nrun = \"cargo fmt\"\n",
            "commandz is unknown; a checks plugin's table needs it in checks",
        ),
        (
            "checks = [\"commands\"]\n[commands.fmt]\nrun = \"\"\n",
            ".atelier/config.toml: commands.fmt.run: must be a command",
        ),
        (
            "[paths.\"../up\"]\nskip = []\n",
            "paths.\"../up\" must be a folder inside the project",
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

    let added = sandbox
        .command(&["add", REPO])
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
