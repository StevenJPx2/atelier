//! Worktrees against real repositories: an `origin` bare repository with a
//! `v2` branch, and atelier's own bare clone of it, made as the daemon
//! makes it.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test support: a fixture that breaks fails the test that uses it"
)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use atelier_core::{
    Kind, Outcomes, Plugin, Repo, Setting, Settings, Step, Tree, WorktreeRequest, Worktrees,
};
use atelier_git::{Git, Repository};

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A folder removed when the test ends.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Whether the worktree at `dir` has a hooks folder of its own.
fn hooks_of(dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["config", "--get", "core.hooksPath"])
        .output()
        .expect("git runs");

    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).unwrap().trim().to_owned())
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

/// `origin`, a seed clone that pushed `v2`, atelier's clone, and the
/// worktrees' root.
struct Fixture {
    scratch: Scratch,
    local: PathBuf,
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
        let repo = Repo("StevenJPx2/chauffeur".into());
        let local = Repository::under(&root.join("clones"), &repo).unwrap();
        local
            .create(&root.join("origin.git").to_string_lossy())
            .unwrap();

        Self {
            local: local.path().to_owned(),
            git: Git::new(root.join("worktrees")),
            repo,
            seed,
            scratch,
        }
    }

    fn request<'a>(&'a self, branch: &'a str) -> WorktreeRequest<'a> {
        WorktreeRequest {
            repo: &self.repo,
            local: &self.local,
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
    run(&fixture.local, &["fetch", "--quiet", "origin", "v2"]);
    let work = run(
        &fixture.local,
        &["commit-tree", "origin/v2:", "-p", "origin/v2", "-m", "work"],
    );
    run(&fixture.local, &["branch", BRANCH, &work]);

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

    fixture.git.remove(&fixture.local, &tree).unwrap();
    fixture.git.remove(&fixture.local, &tree).unwrap();

    assert!(!tree.exists());
    assert_eq!(run(&fixture.local, &["branch", "--list", BRANCH]), BRANCH);
    assert!(!run(&fixture.local, &["worktree", "list"]).contains("issue-12"));
}

#[test]
fn a_clone_is_bare_with_its_origin_and_no_branch_and_another_origin_is_refused() {
    let fixture = Fixture::new();
    let local = Repository::new(&fixture.local);
    let origin = fixture.scratch.0.join("origin.git");

    local.create(&origin.to_string_lossy()).unwrap();
    let other = local.create("https://github.com/StevenJPx2/other.git");
    local.fetch("v2").unwrap();

    assert_eq!(
        fixture.local,
        fixture.scratch.0.join("clones/StevenJPx2/chauffeur.git")
    );
    assert!(local.exists());
    assert_eq!(
        run(&fixture.local, &["rev-parse", "--is-bare-repository"]),
        "true"
    );
    assert_eq!(local.origin().unwrap(), origin.to_string_lossy());
    assert_eq!(
        run(&fixture.local, &["for-each-ref", "--format=%(refname)"]),
        "refs/remotes/origin/v2"
    );
    let error = other.unwrap_err();
    assert!(error.is_permanent());
    assert!(error.message.contains("has origin"), "{error}");
}

#[test]
fn a_worktree_made_from_your_checkout_is_removed_too() {
    let fixture = Fixture::new();
    let root = &fixture.scratch.0;
    run(root, &["clone", "--quiet", "origin.git", "checkout"]);
    let checkout = root.join("checkout");
    let tree = fixture.expected();
    let target = tree.to_string_lossy();
    run(
        &checkout,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            BRANCH,
            &target,
            "origin/v2",
        ],
    );

    fixture.git.remove(&fixture.local, &tree).unwrap();

    assert!(!tree.exists());
    assert!(!run(&checkout, &["worktree", "list"]).contains("issue-12"));
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
        let clone = Repository::under(&fixture.scratch.0.join("clones"), &repo).unwrap_err();

        assert!(error.is_permanent(), "{name}");
        assert!(clone.is_permanent(), "{name}");
    }

    assert!(!fixture.scratch.0.join("worktrees").exists());
}

#[test]
fn a_repository_reads_its_origin_its_default_branch_and_files_there() {
    let fixture = Fixture::new();
    run(&fixture.seed, &["switch", "--quiet", "--create", "main"]);
    std::fs::create_dir_all(fixture.seed.join(".atelier")).unwrap();
    std::fs::write(fixture.seed.join(".atelier/config.toml"), "base = \"v2\"\n").unwrap();
    run(&fixture.seed, &["add", ".atelier/config.toml"]);
    run(&fixture.seed, &["commit", "--quiet", "-m", "config"]);
    run(&fixture.seed, &["push", "--quiet", "origin", "main"]);
    let repository = Repository::new(&fixture.local);

    let origin = repository.origin().unwrap();
    let default = repository.default_branch().unwrap();
    repository.fetch("main").unwrap();
    repository.fetch("v2").unwrap();
    let config = repository.show("main", ".atelier/config.toml").unwrap();
    let missing = repository.show("v2", ".atelier/config.toml").unwrap();
    let unknown = repository.show("nope", ".atelier/config.toml").unwrap_err();
    let unfetchable = repository.fetch("nope").unwrap_err();

    assert!(origin.ends_with("origin.git"), "{origin}");

    let local = fixture.scratch.0.join("origin.git");
    let insteadof = format!("url.{}.insteadOf", local.display());
    run(
        &fixture.local,
        &[
            "config",
            &insteadof,
            "https://github.com/StevenJPx2/chauffeur.git",
        ],
    );
    run(
        &fixture.local,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/StevenJPx2/chauffeur.git",
        ],
    );
    assert_eq!(
        repository.origin().unwrap(),
        "https://github.com/StevenJPx2/chauffeur.git"
    );
    assert_eq!(repository.default_branch().unwrap(), "main");
    assert_eq!(default, "main");
    assert_eq!(config.as_deref(), Some("base = \"v2\"\n"));
    assert_eq!(missing, None);
    assert!(unknown.is_permanent());
    assert!(!unfetchable.is_permanent());
}

#[test]
fn a_branch_tree_lists_and_reads_origin_s_files_as_last_fetched() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.seed.join(".atelier/prompts")).unwrap();
    std::fs::write(
        fixture.seed.join(".atelier/prompts/audit.md"),
        "Audit it.\n",
    )
    .unwrap();
    std::fs::write(fixture.seed.join("README.md"), "# chauffeur\n").unwrap();
    run(&fixture.seed, &["add", "."]);
    run(&fixture.seed, &["commit", "--quiet", "-m", "prompts"]);
    run(&fixture.seed, &["push", "--quiet", "origin", "v2"]);
    let repository = Repository::new(&fixture.local);
    repository.fetch("v2").unwrap();

    let tree = repository.tree("v2").unwrap();
    let unknown = repository.tree("nope").unwrap_err();

    assert_eq!(tree.files(), [".atelier/prompts/audit.md", "README.md"]);
    assert_eq!(
        tree.read(".atelier/prompts/audit.md").as_deref(),
        Some("Audit it.\n")
    );
    assert_eq!(tree.read("missing.md"), None);
    assert_eq!(tree.read(".atelier/prompts"), None);
    assert!(unknown.is_permanent());
}

#[test]
fn git_offers_the_worktree_kind_which_takes_no_settings() {
    let git = Git::new("/worktrees");
    let tree = atelier_core::MemoryTree::default();
    let extra = Settings::default().with("path", Setting::Text("/tmp".into()));

    assert_eq!(git.name(), "git");
    assert_eq!(
        git.kinds(),
        [Kind {
            name: "worktree",
            outcomes: Outcomes::Own(&["passed"]),
            resident: false,
        }]
    );
    assert_eq!(
        git.configure("worktree", &Settings::default(), &tree)
            .unwrap(),
        Settings::default()
    );
    assert_eq!(
        git.configure("worktree", &extra, &tree)
            .unwrap_err()
            .to_string(),
        "path: unknown; it takes no settings"
    );
}
#[test]
fn a_worktree_gets_its_own_hooks_and_the_clone_stays_bare() {
    let fixture = Fixture::new();
    let hooks = fixture.scratch.0.join("hooks");
    let with_hooks = Git::new(fixture.scratch.0.join("worktrees")).with_hooks(&hooks);
    let plain = fixture.git.create(&WorktreeRequest {
        name: "issue-13",
        ..fixture.request("issue-13-plain")
    });

    let tree = with_hooks.create(&fixture.request(BRANCH)).unwrap();
    let again = with_hooks.create(&fixture.request(BRANCH)).unwrap();

    assert_eq!(again, tree);
    assert_eq!(hooks_of(&tree).as_deref(), Some(hooks.to_str().unwrap()));
    assert_eq!(hooks_of(&plain.unwrap()), None);
    assert_eq!(hooks_of(&fixture.local), None);
    assert_eq!(run(&tree, &["rev-parse", "--is-bare-repository"]), "false");
    assert_eq!(
        run(&fixture.local, &["rev-parse", "--is-bare-repository"]),
        "true"
    );
    assert_eq!(run(&tree, &["status", "--short"]), "");
}

#[test]
fn a_reused_worktree_gets_its_hooks_too() {
    let fixture = Fixture::new();
    let hooks = fixture.scratch.0.join("hooks");
    let tree = fixture.git.create(&fixture.request(BRANCH)).unwrap();

    Git::new(fixture.scratch.0.join("worktrees"))
        .with_hooks(&hooks)
        .create(&fixture.request(BRANCH))
        .unwrap();

    assert_eq!(hooks_of(&tree).as_deref(), Some(hooks.to_str().unwrap()));
}

#[test]
fn a_worktree_of_a_checkout_gets_hooks_and_the_checkout_keeps_its_own() {
    let fixture = Fixture::new();
    let root = &fixture.scratch.0;
    run(root, &["clone", "--quiet", "origin.git", "checkout"]);
    let checkout = root.join("checkout");
    let hooks = root.join("hooks");
    let git = Git::new(root.join("worktrees")).with_hooks(&hooks);

    let tree = git
        .create(&WorktreeRequest {
            local: &checkout,
            ..fixture.request(BRANCH)
        })
        .unwrap();

    assert_eq!(hooks_of(&tree).as_deref(), Some(hooks.to_str().unwrap()));
    assert_eq!(hooks_of(&checkout), None);
    assert_eq!(
        run(&checkout, &["rev-parse", "--is-bare-repository"]),
        "false"
    );
    assert_eq!(run(&checkout, &["status", "--short"]), "");
}
