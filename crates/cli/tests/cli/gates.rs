//! `atelier gates`, `check` and `fix` in a checkout, with gates that are
//! real shell commands.

#![expect(
    clippy::unwrap_used,
    reason = "test support: a sandbox that breaks fails the test that uses it"
)]

use std::fs;

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

/// A sandbox whose `main` has the gates above, `Cargo.toml` and a lowercase
/// `docs/README.md`, with the checkout on a branch from it.
fn sandbox() -> Sandbox {
    let sandbox = Sandbox::new(&labelled(&[]));
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
