//! Worktrees against real repositories: an `origin` bare repository with a
//! `v2` branch, and a clone of it as the project's checkout.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test support: a fixture that breaks fails the test that uses it"
)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use atelier_core::{Repo, WorktreeRequest, Worktrees};
use atelier_git::{Checkout, Git};

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A folder removed when the test ends.
struct Scratch(PathBuf);

impl Drop for Scratch {
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

/// `origin`, a seed clone that pushed `v2`, the project's checkout, and
/// the worktrees' root.
struct Fixture {
    scratch: Scratch,
    checkout: PathBuf,
    seed: PathBuf,
    git: Git,
    repo: Repo,
}

impl Fixture {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("atelier-git-{}-{nanos}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let scratch = Scratch(root.canonicalize().unwrap());
        let root = scratch.0.clone();

        run(
            &root,
            &["init", "--bare", "--initial-branch", "main", "origin.git"],
        );
        run(&root, &["clone", "--quiet", "origin.git", "seed"]);
        let seed = root.join("seed");
        run(&seed, &["switch", "--quiet", "--create", "v2"]);
        run(&seed, &["commit", "--quiet", "--allow-empty", "-m", "base"]);
        run(&seed, &["push", "--quiet", "origin", "v2"]);
        run(
            &root,
            &[
                "clone",
                "--quiet",
                "--branch",
                "v2",
                "origin.git",
                "checkout",
            ],
        );

        Self {
            checkout: root.join("checkout"),
            git: Git::new(root.join("worktrees")),
            repo: Repo("StevenJPx2/chauffeur".into()),
            seed,
            scratch,
        }
    }

    fn request<'a>(&'a self, branch: &'a str) -> WorktreeRequest<'a> {
        WorktreeRequest {
            repo: &self.repo,
            checkout: &self.checkout,
            name: "issue-12",
            branch,
            base: "v2",
        }
    }

    fn expected(&self) -> PathBuf {
        self.scratch
            .0
            .join("worktrees/StevenJPx2/chauffeur/issue-12")
    }
}

const BRANCH: &str = "issue-12-fix-the-dead-link";

#[test]
fn a_new_branch_starts_from_the_base_on_origin_and_is_reused() {
    let fixture = Fixture::new();
    run(
        &fixture.seed,
        &["commit", "--quiet", "--allow-empty", "-m", "newer base"],
    );
    run(&fixture.seed, &["push", "--quiet", "origin", "v2"]);
    let newest = run(&fixture.seed, &["rev-parse", "HEAD"]);

    let tree = fixture.git.create(&fixture.request(BRANCH)).unwrap();
    let again = fixture.git.create(&fixture.request(BRANCH)).unwrap();

    assert_eq!(tree, fixture.expected());
    assert_eq!(again, tree);
    assert_eq!(run(&tree, &["rev-parse", "--abbrev-ref", "HEAD"]), BRANCH);
    assert_eq!(run(&tree, &["rev-parse", "HEAD"]), newest);
}

#[test]
fn a_local_branch_keeps_its_commits() {
    let fixture = Fixture::new();
    run(&fixture.checkout, &["branch", BRANCH]);
    run(&fixture.checkout, &["switch", "--quiet", BRANCH]);
    run(
        &fixture.checkout,
        &["commit", "--quiet", "--allow-empty", "-m", "work"],
    );
    let work = run(&fixture.checkout, &["rev-parse", "HEAD"]);
    run(&fixture.checkout, &["switch", "--quiet", "v2"]);

    let tree = fixture.git.create(&fixture.request(BRANCH)).unwrap();

    assert_eq!(run(&tree, &["rev-parse", "HEAD"]), work);
}

#[test]
fn a_branch_only_on_origin_is_continued_and_tracked() {
    let fixture = Fixture::new();
    run(&fixture.seed, &["switch", "--quiet", "--create", BRANCH]);
    run(
        &fixture.seed,
        &["commit", "--quiet", "--allow-empty", "-m", "pushed work"],
    );
    run(&fixture.seed, &["push", "--quiet", "origin", BRANCH]);
    let pushed = run(&fixture.seed, &["rev-parse", "HEAD"]);

    let tree = fixture.git.create(&fixture.request(BRANCH)).unwrap();

    assert_eq!(run(&tree, &["rev-parse", "HEAD"]), pushed);
    assert_eq!(
        run(&tree, &["rev-parse", "--abbrev-ref", "@{upstream}"]),
        format!("origin/{BRANCH}")
    );
}

#[test]
fn a_folder_on_another_branch_is_refused() {
    let fixture = Fixture::new();
    fixture.git.create(&fixture.request(BRANCH)).unwrap();

    let error = fixture
        .git
        .create(&fixture.request("issue-12-other"))
        .unwrap_err();

    assert!(error.is_permanent());
    assert!(
        error
            .message
            .contains(&format!("is on {BRANCH}, not issue-12-other")),
        "{error}"
    );
}

#[test]
fn removing_drops_the_worktree_and_its_changes_but_keeps_the_branch() {
    let fixture = Fixture::new();
    let tree = fixture.git.create(&fixture.request(BRANCH)).unwrap();
    std::fs::write(tree.join("scratch.txt"), "left behind").unwrap();

    fixture.git.remove(&fixture.checkout, &tree).unwrap();
    fixture.git.remove(&fixture.checkout, &tree).unwrap();

    assert!(!tree.exists());
    assert_eq!(
        run(&fixture.checkout, &["branch", "--list", BRANCH]),
        BRANCH
    );
    assert!(!run(&fixture.checkout, &["worktree", "list"]).contains("issue-12"));
}

#[test]
fn a_repository_name_that_could_leave_the_root_is_refused() {
    let fixture = Fixture::new();

    for name in ["../escape", "owner", "a/b/c", "owner/..", "owner/a b"] {
        let repo = Repo(name.into());
        let request = WorktreeRequest {
            repo: &repo,
            ..fixture.request(BRANCH)
        };

        let error = fixture.git.create(&request).unwrap_err();

        assert!(error.is_permanent(), "{name}");
    }

    assert!(!fixture.scratch.0.join("worktrees").exists());
}

#[test]
fn a_checkout_reads_its_origin_its_default_branch_and_files_there() {
    let fixture = Fixture::new();
    run(&fixture.seed, &["switch", "--quiet", "--create", "main"]);
    std::fs::create_dir_all(fixture.seed.join(".atelier")).unwrap();
    std::fs::write(fixture.seed.join(".atelier/config.toml"), "base = \"v2\"\n").unwrap();
    run(&fixture.seed, &["add", ".atelier/config.toml"]);
    run(&fixture.seed, &["commit", "--quiet", "-m", "config"]);
    run(&fixture.seed, &["push", "--quiet", "origin", "main"]);
    let checkout = Checkout::new(&fixture.checkout);

    let origin = checkout.origin().unwrap();
    let default = checkout.default_branch().unwrap();
    checkout.fetch("main").unwrap();
    let config = checkout.show("main", ".atelier/config.toml").unwrap();
    let missing = checkout.show("v2", ".atelier/config.toml").unwrap();
    let unknown = checkout.show("nope", ".atelier/config.toml").unwrap_err();
    let unfetchable = checkout.fetch("nope").unwrap_err();

    assert!(origin.ends_with("origin.git"), "{origin}");

    let local = fixture.scratch.0.join("origin.git");
    let insteadof = format!("url.{}.insteadOf", local.display());
    run(
        &fixture.checkout,
        &[
            "config",
            &insteadof,
            "https://github.com/StevenJPx2/chauffeur.git",
        ],
    );
    run(
        &fixture.checkout,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/StevenJPx2/chauffeur.git",
        ],
    );
    assert_eq!(
        checkout.origin().unwrap(),
        "https://github.com/StevenJPx2/chauffeur.git"
    );
    assert_eq!(checkout.default_branch().unwrap(), "main");
    assert_eq!(default, "main");
    assert_eq!(config.as_deref(), Some("base = \"v2\"\n"));
    assert_eq!(missing, None);
    assert!(unknown.is_permanent());
    assert!(!unfetchable.is_permanent());
}
