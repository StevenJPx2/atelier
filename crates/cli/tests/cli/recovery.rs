//! The daemon after a fault: a clone left half made.

use std::fs;

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
