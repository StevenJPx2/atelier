//! The daemon after a fault: stopped while starting a builder, a builder's
//! session deleted, a clone left half made.

use std::fs;

use serde_json::json;

use crate::labelled;
use crate::sandbox::{Sandbox, git};

fn sessions(sandbox: &Sandbox) -> usize {
    sandbox
        .opencode_calls()
        .iter()
        .filter(|call| call.starts_with("api post /api/session --data"))
        .count()
}

fn building(sandbox: &Sandbox) -> bool {
    sandbox.phase("12").as_deref() == Some("building")
}

#[test]
fn a_daemon_stopped_while_starting_a_builder_starts_no_second_one() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    fs::write(sandbox.bin.join("slow"), "").unwrap();

    let daemon = sandbox.daemon();
    sandbox.wait("a session made", |sandbox| sessions(sandbox) == 1);
    drop(daemon);
    fs::remove_file(sandbox.bin.join("slow")).unwrap();
    let restarted = sandbox.daemon();
    sandbox.wait("a builder", building);
    drop(restarted);

    assert_eq!(sessions(&sandbox), 1, "{:?}", sandbox.opencode_calls());
}

#[test]
fn an_issue_whose_builder_session_was_deleted_still_stops() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    let daemon = sandbox.daemon();
    sandbox.wait("a builder", building);

    fs::write(sandbox.bin.join("gone"), "").unwrap();
    sandbox.set("12", "state", json!("closed"));
    sandbox.wait("the stop", |sandbox| {
        sandbox.phase("12").as_deref() == Some("stopped")
    });
    drop(daemon);

    assert_eq!(sandbox.world()["issues"]["12"]["labels"], json!([]));
}

#[test]
fn a_clone_left_without_its_origin_is_repaired() {
    let sandbox = Sandbox::new(&labelled(&["atelier:build"]));
    let clone = sandbox.clone();
    fs::remove_dir_all(&clone).unwrap();
    git(
        &sandbox.root,
        &["init", "--quiet", "--bare", &clone.to_string_lossy()],
    );

    let daemon = sandbox.daemon();
    sandbox.wait("a builder", building);
    drop(daemon);

    assert_eq!(
        git(&clone, &["config", "--get", "remote.origin.url"]),
        "https://github.com/StevenJPx2/demo.git"
    );
}
#[test]
fn a_whole_clone_is_used_as_it_is() {
    let sandbox = Sandbox::new(&labelled(&[]));
    sandbox.config("poll = 15\n");
    let asked = |sandbox: &Sandbox| {
        sandbox
            .gh_calls()
            .iter()
            .filter(|call| call[..2] == ["config", "get"])
            .count()
    };
    let listings = |sandbox: &Sandbox| {
        sandbox
            .gh_calls()
            .iter()
            .filter(|call| call.get(2).is_some_and(|path| path.contains("/issues?")))
            .count()
    };

    let daemon = sandbox.daemon();
    sandbox.wait("two polls", |sandbox| listings(sandbox) >= 4);
    drop(daemon);

    assert_eq!(asked(&sandbox), 1, "{:?}", sandbox.gh_calls());
}
