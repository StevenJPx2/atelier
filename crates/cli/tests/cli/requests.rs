//! `finish` and `stop` through the binary and a running worker: the
//! commands leave requests, and only the worker writes the state.

use std::fs;

use crate::labelled;
use crate::sandbox::{REPO, Sandbox};

fn worktree(sandbox: &Sandbox) -> std::path::PathBuf {
    sandbox
        .home
        .join(".local/share/atelier/worktrees/StevenJPx2/demo/issue-12")
}

#[test]
fn finish_queues_an_outcome_without_writing_state_and_the_worker_closes_the_run() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    sandbox.config("poll = 15\n");
    let daemon = sandbox.daemon();
    sandbox.wait("builder", Sandbox::building);
    drop(daemon);
    let state = sandbox.home.join(".local/state/atelier/state.json");
    let before = fs::read(&state).unwrap();
    let worktree = worktree(&sandbox);

    let finished = sandbox.atelier(&[
        "finish",
        "nothing",
        "the link is already correct",
        "--path",
        &worktree.to_string_lossy(),
    ]);

    assert!(
        finished.status.success(),
        "{}",
        String::from_utf8_lossy(&finished.stderr)
    );
    assert_eq!(fs::read(&state).unwrap(), before);
    assert!(String::from_utf8_lossy(&finished.stdout).contains("builder settled nothing"));
    let daemon = sandbox.daemon();
    sandbox.wait("done", |sandbox| {
        sandbox.phase("12").as_deref() == Some("done")
    });
    drop(daemon);

    let issue = &sandbox.world()["issues"]["12"];
    let report = issue["comments"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(issue["state"], "closed");
    assert!(
        report.contains("builder`: nothing (the link is already correct)"),
        "{report}"
    );
    assert!(!worktree.exists());
    let already_done = sandbox.atelier(&["stop", "build github#12"]);
    assert!(!already_done.status.success());
    assert!(
        !sandbox
            .home
            .join(".local/state/atelier/requests/StevenJPx2/demo/0.json")
            .exists()
    );
}

#[test]
fn stop_queues_a_request_and_interrupts_a_running_builder() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    sandbox.config("poll = 15\n");
    let daemon = sandbox.daemon();
    sandbox.wait("builder", Sandbox::building);
    let worktree = worktree(&sandbox);

    let stopped = sandbox.atelier(&["stop", "build github#12", "--repo", REPO]);
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    // The state is saved before the poll writes its report to the log.
    // Keep the daemon alive until both observations have arrived.
    sandbox.wait("stopped and reported", |sandbox| {
        sandbox.phase("12").as_deref() == Some("stopped")
            && sandbox.log().contains("Running → Stopped: stop requested")
    });
    drop(daemon);

    assert!(!worktree.exists());
    assert_eq!(
        sandbox.world()["issues"]["12"]["labels"],
        serde_json::json!([])
    );
    assert!(
        sandbox
            .opencode_calls()
            .iter()
            .any(|call| call.contains("/interrupt"))
    );
    assert!(sandbox.log().contains("Running → Stopped: stop requested"));
}

#[test]
fn finish_refuses_an_outcome_the_running_step_does_not_declare() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    let daemon = sandbox.daemon();
    sandbox.wait("builder", Sandbox::building);
    drop(daemon);
    let path = worktree(&sandbox);

    let refused = sandbox.atelier(&[
        "finish",
        "clean",
        "checked it",
        "--path",
        &path.to_string_lossy(),
    ]);

    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr)
            .contains("no running agent step declares outcome clean")
    );
    assert!(!sandbox.home.join(".local/state/atelier/requests").exists());
}
#[test]
fn finish_requires_a_step_when_two_running_agents_declare_the_same_outcome() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    sandbox.commit(".atelier/audit.md", "Audit the dependencies.");
    sandbox.config(
        r#"
[[pipeline.build.step]]
id = "worktree"
uses = "git.worktree"

[[pipeline.build.step]]
id = "audit"
uses = "agent.session"
prompt = ".atelier/audit.md"

[[pipeline.build.step]]
id = "peer"
uses = "agent.session"
prompt = ".atelier/audit.md"
after = ["worktree"]
"#,
    );
    let daemon = sandbox.daemon();
    sandbox.wait("both agents", |sandbox| {
        sandbox.step("12", "audit").as_deref() == Some("running")
            && sandbox.step("12", "peer").as_deref() == Some("running")
    });
    drop(daemon);
    let path = worktree(&sandbox);
    let where_at = path.to_string_lossy();

    let ambiguous = sandbox.atelier(&["finish", "passed", "clean", "--path", &where_at]);
    let explicit = sandbox.atelier(&[
        "finish", "passed", "clean", "--step", "audit", "--path", &where_at,
    ]);

    assert!(!ambiguous.status.success());
    assert!(String::from_utf8_lossy(&ambiguous.stderr).contains("matches audit, peer; use --step"));
    assert!(
        explicit.status.success(),
        "{}",
        String::from_utf8_lossy(&explicit.stderr)
    );
    let queue = sandbox
        .home
        .join(".local/state/atelier/requests/StevenJPx2/demo/0.json");
    let request: serde_json::Value = serde_json::from_slice(&fs::read(queue).unwrap()).unwrap();
    assert_eq!(request["command"]["step"], "audit");
    assert_eq!(request["command"]["outcome"], "passed");
}

#[test]
fn finish_outside_a_run_and_stop_with_no_matching_run_are_refused() {
    let sandbox = Sandbox::new(&labelled(&[]));

    let finish = sandbox.atelier(&[
        "finish",
        "passed",
        "clean",
        "--path",
        &sandbox.checkout.to_string_lossy(),
    ]);
    let stop = sandbox.atelier(&["stop", "build github#12"]);

    assert!(!finish.status.success());
    assert!(String::from_utf8_lossy(&finish.stderr).contains("no working run owns this worktree"));
    assert!(!stop.status.success());
    assert!(String::from_utf8_lossy(&stop.stderr).contains("no working run has that key"));
}
#[test]
fn finish_requires_both_a_working_run_and_its_matching_worktree() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    let daemon = sandbox.daemon();
    sandbox.wait("builder", Sandbox::building);
    drop(daemon);
    let path = worktree(&sandbox);
    let state_path = sandbox.home.join(".local/state/atelier/state.json");
    let mut state = sandbox.state();

    let elsewhere = sandbox.atelier(&[
        "finish",
        "nothing",
        "clean",
        "--path",
        &sandbox.checkout.to_string_lossy(),
    ]);
    assert!(!elsewhere.status.success());
    assert!(
        String::from_utf8_lossy(&elsewhere.stderr).contains("no working run owns this worktree")
    );

    // Keep the real worktree and running step from the saved run, while
    // representing an inactive run before its cleanup removes the tree.
    for phase in [
        serde_json::json!({"name": "untracked"}),
        serde_json::json!({"name": "done"}),
        serde_json::json!({"name": "blocked", "reason": {"name": "claim"}}),
        serde_json::json!({"name": "stopped"}),
    ] {
        state["projects"][REPO]["runs"][0]["phase"] = phase;
        fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();

        let inactive = sandbox.atelier(&[
            "finish",
            "nothing",
            "clean",
            "--path",
            &path.to_string_lossy(),
        ]);

        assert!(!inactive.status.success());
        assert!(
            String::from_utf8_lossy(&inactive.stderr).contains("no working run owns this worktree")
        );
    }

    assert!(!sandbox.home.join(".local/state/atelier/requests").exists());
}

#[test]
fn stopping_a_run_cancels_its_background_command_group() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    sandbox.config(
        r#"
poll = 15
[[pipeline.build.step]]
id = "worktree"
uses = "git.worktree"

[[pipeline.build.step]]
id = "wait"
uses = "atelier.command"
run = "sleep 300 & wait"
"#,
    );
    let daemon = sandbox.daemon();
    sandbox.wait("command running", |sandbox| {
        sandbox.step("12", "wait").as_deref() == Some("running")
    });
    let group = sandbox
        .home
        .join(".local/state/atelier/steps/StevenJPx2/demo/issue-12/build/1/wait/group");
    let group = fs::read_to_string(group).unwrap();
    let stopped = sandbox.atelier(&["stop", "build github#12"]);
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    sandbox.wait("stopped", |sandbox| {
        sandbox.phase("12").as_deref() == Some("stopped")
    });
    drop(daemon);

    let alive = std::process::Command::new("kill")
        .args(["-0", "--", &format!("-{}", group.trim())])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();
    if alive {
        std::process::Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", group.trim())])
            .status()
            .unwrap();
    }

    assert!(!alive, "the command process group survived stop");
    assert!(!worktree(&sandbox).exists());
}
#[test]
fn finish_checks_reason_bounds_and_stop_requires_the_exact_run_key() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    let daemon = sandbox.daemon();
    sandbox.wait("builder", Sandbox::building);
    drop(daemon);
    let path = worktree(&sandbox);
    let where_at = path.to_string_lossy();

    let empty = sandbox.atelier(&["finish", "nothing", " ", "--path", &where_at]);
    let oversized = sandbox.atelier(&["finish", "nothing", &"x".repeat(4097), "--path", &where_at]);
    let exact = sandbox.atelier(&["finish", "nothing", &"x".repeat(4096), "--path", &where_at]);
    let wrong_run = sandbox.atelier(&["stop", "other github#12"]);

    assert!(!empty.status.success());
    assert!(!oversized.status.success());
    assert!(
        exact.status.success(),
        "{}",
        String::from_utf8_lossy(&exact.stderr)
    );
    assert!(!wrong_run.status.success());
}
