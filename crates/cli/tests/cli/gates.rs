//! `atelier gates`, `check` and `fix` in a checkout, with gates that are
//! real shell commands.

#![expect(
    clippy::unwrap_used,
    reason = "test support: a sandbox that breaks fails the test that uses it"
)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use crate::labelled;
use crate::sandbox::{Sandbox, git};

const CONFIG: &str = r#"checks = ["commands"]

[commands.build]
run = "test -f Cargo.toml"

[commands.prepared]
setup = ["touch .prepared"]
run = "test -f .prepared"

[commands.base]
run = "printf %s \"$ATELIER_BASE\" > .base"

[commands.lint]
root = "docs"
run = "! grep -n '[a-z]' README.md"
fix = "tr a-z A-Z < README.md > .upper && mv .upper README.md"
"#;

/// A sandbox whose `main` has the gates above, `Cargo.toml` and an
/// uppercase `docs/README.md`, with the checkout on a branch from it.
fn sandbox() -> Sandbox {
    sandbox_labelled(&[])
}

fn sandbox_labelled(labels: &[&str]) -> Sandbox {
    let sandbox = Sandbox::new(&labelled(labels));
    sandbox.config(CONFIG);
    let checkout = &sandbox.checkout;
    git(checkout, &["pull", "--quiet", "origin", "main"]);
    write(&sandbox, "Cargo.toml", "[workspace]\n");
    write(&sandbox, "docs/README.md", "DOCS\n");
    write(&sandbox, ".gitignore", ".prepared\n.base\n");
    git(checkout, &["add", "."]);
    git(checkout, &["commit", "--quiet", "-m", "files"]);
    git(checkout, &["push", "--quiet", "origin", "main"]);
    git(checkout, &["switch", "--quiet", "-c", "work"]);
    sandbox
}

fn write(sandbox: &Sandbox, path: &str, text: &str) {
    let path = sandbox.checkout.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn run(sandbox: &Sandbox, args: &[&str]) -> (bool, String) {
    let path = sandbox.checkout.to_string_lossy().into_owned();
    let mut args = args.to_vec();
    args.extend(["--path", &path]);
    let output = sandbox.atelier(&args);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    (output.status.success(), text)
}

#[test]
fn gates_lists_each_root_and_its_gates_and_the_matrix_for_a_diff() {
    let sandbox = sandbox();
    write(&sandbox, "docs/README.md", "Docs\n");

    let (listed, all) = run(&sandbox, &["gates"]);
    let (_, matrix) = run(&sandbox, &["gates", "--json", "--diff", "origin/main"]);

    assert!(listed, "{all}");
    assert_eq!(
        all,
        ".     commands/base\n.     commands/build\n.     commands/prepared\ndocs  commands/lint\n"
    );
    assert_eq!(matrix, "[{\"gate\":\"commands/lint\",\"root\":\"docs\"}]\n");
}

#[test]
fn check_runs_the_gates_the_changes_touch_and_a_failure_exits_1() {
    let sandbox = sandbox();
    write(&sandbox, "docs/README.md", "Docs, in lowercase\n");

    let (passed, output) = run(&sandbox, &["check"]);

    assert!(!passed, "{output}");
    assert!(output.starts_with("docs  commands/lint  fail"), "{output}");
    assert!(output.contains("\n    1:Docs, in lowercase\n"), "{output}");
    assert!(!output.contains("commands/build"), "{output}");
}

#[test]
fn check_runs_one_gate_by_name_with_its_setup_and_the_base() {
    let sandbox = sandbox();
    let base = sandbox.checkout.join(".base");

    let (unprepared, _) = run(
        &sandbox,
        &["check", "--gate", "commands/prepared", "--root", "."],
    );
    let (prepared, output) = run(
        &sandbox,
        &[
            "check",
            "--setup",
            "--gate",
            "commands/prepared",
            "--root",
            ".",
        ],
    );
    run(
        &sandbox,
        &["check", "--gate", "commands/base", "--root", "."],
    );
    let default = fs::read_to_string(&base).unwrap();
    run(
        &sandbox,
        &[
            "check",
            "--diff",
            "HEAD~1",
            "--gate",
            "commands/base",
            "--root",
            ".",
        ],
    );
    let given = fs::read_to_string(&base).unwrap();
    let (unknown, refusal) = run(
        &sandbox,
        &["check", "--gate", "commands/nope", "--root", "."],
    );

    assert!(!unprepared);
    assert!(prepared, "{output}");
    assert!(output.starts_with(".  commands/prepared  pass"), "{output}");
    assert_eq!(default, "origin/main");
    assert_eq!(given, "HEAD~1");
    assert!(!unknown);
    assert!(refusal.contains("no gate commands/nope in ."), "{refusal}");
}

#[test]
fn fix_staged_fixes_whole_files_and_stages_them_again() {
    let sandbox = sandbox();
    let checkout = &sandbox.checkout;
    write(&sandbox, "docs/README.md", "fixed when staged\n");
    write(&sandbox, "docs/guide.md", "staged\n");
    git(checkout, &["add", "docs/README.md", "docs/guide.md"]);
    write(&sandbox, "docs/guide.md", "staged, then edited\n");

    let (fixed, output) = run(&sandbox, &["fix", "--staged"]);

    assert!(fixed, "{output}");
    assert_eq!(output, "docs  commands/lint  fixed\n");
    assert_eq!(
        git(checkout, &["show", ":docs/README.md"]),
        "FIXED WHEN STAGED"
    );
    assert_eq!(git(checkout, &["show", ":docs/guide.md"]), "staged");
}

#[test]
fn a_gate_added_on_the_branch_is_not_run_until_it_is_on_the_base() {
    let sandbox = sandbox();
    write(
        &sandbox,
        ".atelier/config.toml",
        &format!("{CONFIG}\n[commands.extra]\nrun = \"false\"\n"),
    );
    git(
        &sandbox.checkout,
        &["commit", "--quiet", "-am", "a gate of its own"],
    );

    let (_, listed) = run(&sandbox, &["gates"]);

    assert!(!listed.contains("commands/extra"), "{listed}");
}
/// A git command as a builder runs it: with the sandbox's home, so
/// `github.com` is the sandbox's own, and no `atelier` on `PATH`.
fn builder_git(sandbox: &Sandbox, dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("HOME", &sandbox.home)
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_AUTHOR_NAME", "builder")
        .env("GIT_AUTHOR_EMAIL", "builder@example.com")
        .env("GIT_COMMITTER_NAME", "builder")
        .env("GIT_COMMITTER_EMAIL", "builder@example.com")
        .output()
        .unwrap()
}

#[test]
fn a_builders_commit_is_fixed_by_its_hook_and_the_projects_hook_still_runs() {
    let sandbox = sandbox_labelled(&["atelier:build"]);
    let checkout = &sandbox.checkout;
    git(checkout, &["switch", "--quiet", "main"]);
    write(
        &sandbox,
        ".githooks/pre-commit",
        "#!/bin/sh\ntouch \"$(git rev-parse --show-toplevel)/.ran\"\n",
    );
    let own = checkout.join(".githooks/pre-commit");
    fs::set_permissions(&own, fs::Permissions::from_mode(0o755)).unwrap();
    git(checkout, &["add", ".githooks"]);
    git(checkout, &["commit", "--quiet", "-m", "the project's hook"]);
    git(checkout, &["push", "--quiet", "origin", "main"]);

    let daemon = sandbox.daemon();
    sandbox.wait("a builder", Sandbox::building);
    drop(daemon);

    let worktree = sandbox
        .home
        .join(".local/share/atelier/worktrees/StevenJPx2/demo/issue-12");
    let hook = sandbox.home.join(".local/share/atelier/hooks/pre-commit");
    git(&sandbox.clone(), &["config", "core.hooksPath", ".githooks"]);
    fs::write(worktree.join("docs/README.md"), "fixed by the hook\n").unwrap();
    builder_git(&sandbox, &worktree, &["add", "docs/README.md"]);

    let committed = builder_git(&sandbox, &worktree, &["commit", "--quiet", "-m", "docs"]);

    let said = String::from_utf8_lossy(&committed.stderr).into_owned();
    let hooks_of = |dir: &Path| {
        String::from_utf8(builder_git(&sandbox, dir, &["config", "--get", "core.hooksPath"]).stdout)
            .unwrap()
    };
    assert!(committed.status.success(), "{said}");
    assert_eq!(
        git(&worktree, &["show", "HEAD:docs/README.md"]),
        "FIXED BY THE HOOK"
    );
    assert!(
        worktree.join(".ran").exists(),
        "the project's hook did not run"
    );
    assert_eq!(
        fs::metadata(&hook).unwrap().permissions().mode() & 0o111,
        0o111
    );
    assert_eq!(
        hooks_of(&worktree).trim(),
        hook.parent().unwrap().to_str().unwrap()
    );
    assert_eq!(hooks_of(checkout), "");
    assert_eq!(hooks_of(&sandbox.clone()).trim(), ".githooks");
}
