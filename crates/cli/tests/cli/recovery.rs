//! The daemon after a fault: a builder's session deleted, a clone left half
//! made.

use std::fs;

use serde_json::json;

use crate::labelled;
use crate::sandbox::{Sandbox, git};

fn building(sandbox: &Sandbox) -> bool {
    sandbox.phase("12").as_deref() == Some("building")
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
