//! A checkout's working tree, against a real repository.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test support: a fixture that breaks fails the test that uses it"
)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use atelier_core::Tree;
use atelier_git::{Staged, WorkingTree};

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A checkout removed when the test ends.
struct Checkout(PathBuf);

impl Drop for Checkout {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "atelier")
        .env("GIT_AUTHOR_EMAIL", "atelier@example.com")
        .env("GIT_COMMITTER_NAME", "atelier")
        .env("GIT_COMMITTER_EMAIL", "atelier@example.com")
        .output()
        .expect("git runs");

    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn write(dir: &Path, path: &str, text: &str) {
    let path = dir.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A checkout whose `main` has `Cargo.toml`, `docs/a.md`, `docs/b.md` and
/// a `.gitignore` of `target/`.
fn checkout() -> Checkout {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("atelier-tree-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();

    run(&root, &["init", "--quiet", "--initial-branch", "main"]);
    for (path, text) in [
        ("Cargo.toml", "[workspace]\n"),
        ("docs/a.md", "# A\n"),
        ("docs/b.md", "# B\n"),
        (".gitignore", "target/\n"),
    ] {
        write(&root, path, text);
    }
    run(&root, &["add", "."]);
    run(&root, &["commit", "--quiet", "-m", "base"]);

    Checkout(root)
}

#[test]
fn it_lists_tracked_and_untracked_files_but_not_ignored_ones() {
    let checkout = checkout();
    let root = &checkout.0;
    write(root, "src/new.rs", "fn main() {}\n");
    write(root, "target/debug/out", "built");

    let tree = WorkingTree::read(root).unwrap();
    let from_docs = WorkingTree::read(&root.join("docs")).unwrap();

    assert_eq!(tree.root(), root.as_path());
    assert_eq!(from_docs, tree);
    assert_eq!(
        tree.files(),
        [
            ".gitignore",
            "Cargo.toml",
            "docs/a.md",
            "docs/b.md",
            "src/new.rs"
        ]
    );
    assert_eq!(tree.read("docs/a.md").as_deref(), Some("# A\n"));
    assert_eq!(tree.read("docs/missing.md"), None);
    assert_eq!(tree.read("../outside"), None);
    assert_eq!(tree.read("/etc/hosts"), None);
    assert_eq!(tree.read("docs/../Cargo.toml"), None);
}

#[test]
fn changed_counts_commits_since_the_base_and_every_uncommitted_change() {
    let checkout = checkout();
    let root = &checkout.0;
    run(root, &["switch", "--quiet", "-c", "issue-1"]);
    write(root, "docs/a.md", "# A, changed\n");
    run(root, &["commit", "--quiet", "-am", "work"]);
    write(root, "Cargo.toml", "[workspace]\nmembers = []\n");
    run(root, &["add", "Cargo.toml"]);
    write(root, "docs/b.md", "# B, unstaged\n");
    write(root, "src/new.rs", "fn main() {}\n");
    write(root, "target/ignored", "x");

    let tree = WorkingTree::read(root).unwrap();
    let changed = tree.changed("main").unwrap();
    let unknown = tree.changed("no-such-branch").unwrap_err();

    assert_eq!(
        changed,
        ["Cargo.toml", "docs/a.md", "docs/b.md", "src/new.rs"]
    );
    assert!(unknown.is_permanent());
}

#[test]
fn staged_files_are_told_apart_from_partly_staged_ones_and_staged_again() {
    let checkout = checkout();
    let root = &checkout.0;
    write(root, "docs/a.md", "# A, staged\n");
    write(root, "docs/b.md", "# B, staged\n");
    run(root, &["add", "docs/a.md", "docs/b.md"]);
    write(root, "docs/b.md", "# B, staged, then edited\n");
    run(root, &["rm", "--quiet", "Cargo.toml"]);
    let tree = WorkingTree::read(root).unwrap();

    let staged = tree.staged().unwrap();
    tree.stage(&["docs/b.md".to_owned()]).unwrap();
    tree.stage(&[]).unwrap();
    let after = tree.staged().unwrap();

    assert_eq!(
        staged,
        Staged {
            files: vec!["docs/a.md".into(), "docs/b.md".into()],
            partly: vec!["docs/b.md".into()],
        }
    );
    assert_eq!(after.partly, Vec::<String>::new());
    assert!(tree.stage(&["docs/missing.md".to_owned()]).is_err());
}

#[test]
fn a_folder_that_is_no_checkout_is_refused() {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let plain = std::env::temp_dir().join(format!("atelier-plain-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&plain).unwrap();

    let error = WorkingTree::read(&plain).unwrap_err();
    let _ = std::fs::remove_dir_all(&plain);

    assert!(error.is_permanent());
}
