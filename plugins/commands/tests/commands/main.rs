//! The `commands` plugin through its settings, the registry and core's
//! choice of gates.

use atelier_commands::{Commands, Listed};
use atelier_core::{Checks, MemoryTree, Overrides, Registry, Root, Setting, Settings, gates};

fn text(value: &str) -> Setting {
    Setting::Text(value.into())
}

fn gate(entries: &[(&str, Setting)]) -> Setting {
    let table = entries
        .iter()
        .fold(Settings::default(), |table, (key, value)| {
            table.with(key, value.clone())
        });

    Setting::Table(table)
}

fn listed() -> Settings {
    Settings::default()
        .with(
            "clippy",
            gate(&[
                ("run", text("cargo clippy -- -D warnings")),
                (
                    "setup",
                    Setting::List(vec![text("rustup component add clippy")]),
                ),
                ("fix", text("cargo clippy --fix --allow-dirty")),
            ]),
        )
        .with(
            "deploy",
            gate(&[("run", text("wrangler deploy")), ("root", text("site"))]),
        )
        .with(
            "lint",
            gate(&[("run", text("markdownlint .")), ("root", text("docs"))]),
        )
        .with(
            "prettier",
            gate(&[
                ("run", text("prettier --check .")),
                ("root", text("docs")),
                ("fix", text("prettier --write .")),
            ]),
        )
}

fn tree() -> MemoryTree {
    MemoryTree::new([
        ("Cargo.toml", "[workspace]"),
        ("docs/README.md", "# Docs"),
        ("sitemap.xml", "<urlset/>"),
    ])
}

#[test]
fn each_gate_runs_in_its_root_with_its_setup_and_fix() {
    let listed = Listed::from_settings(&listed()).unwrap();
    let tree = tree();
    let top = Root(".".into());
    let docs = Root("docs".into());

    let roots = listed.roots(&tree);
    let top_gates = listed.gates(&tree, &top);
    let docs_gates: Vec<String> = listed
        .gates(&tree, &docs)
        .into_iter()
        .map(|gate| format!("{} {}", gate.name, gate.run))
        .collect();
    let docs_fixes: Vec<String> = listed
        .fixes(&tree, &docs)
        .into_iter()
        .map(|fix| format!("{} {}", fix.name, fix.run))
        .collect();

    assert_eq!(roots, [top.clone(), docs]);
    assert_eq!(top_gates.len(), 1);
    assert_eq!(top_gates[0].name, "clippy");
    assert_eq!(top_gates[0].setup, ["rustup component add clippy"]);
    assert_eq!(top_gates[0].run, "cargo clippy -- -D warnings");
    assert_eq!(
        docs_gates,
        ["lint markdownlint .", "prettier prettier --check ."]
    );
    assert_eq!(docs_fixes, ["prettier prettier --write ."]);
    assert_eq!(
        listed.fixes(&tree, &top)[0].run,
        "cargo clippy --fix --allow-dirty"
    );
    assert_eq!(listed.gates(&tree, &Root("site".into())).len(), 1);
}

#[test]
fn it_runs_only_where_a_project_names_it() {
    let registry = Registry::new(&[&Commands]);
    let wanted = ["commands".to_owned()];
    let unnamed = registry.find_checks(&[]).unwrap();
    let runs = registry.find_checks(&wanted).unwrap();
    let checks = runs[0].1.make(&listed()).unwrap();
    let changed = ["docs/README.md".to_owned()];

    let planned = gates(
        &[(runs[0].0, checks.as_ref())],
        &tree(),
        &Overrides::default(),
        Some(&changed),
    );

    let names: Vec<String> = planned
        .iter()
        .map(|gate| format!("{} {}", gate.root.0, gate.name()))
        .collect();
    assert!(unnamed.is_empty());
    assert_eq!(names, ["docs commands/lint", "docs commands/prettier"]);
}

#[test]
fn a_gate_it_cannot_use_is_refused_by_its_path() {
    let refused = |name: &str, value: Setting| {
        Listed::from_settings(&Settings::default().with(name, value))
            .unwrap_err()
            .to_string()
    };
    let run = || ("run", text("make"));

    assert_eq!(
        refused("make", text("make")),
        "make: must be a table with run"
    );
    assert_eq!(
        refused("make it", gate(&[run()])),
        "make it: a gate's name takes letters, digits, - and _"
    );
    assert_eq!(
        refused("make", gate(&[run(), ("args", text("-j"))])),
        "make.args: unknown; it takes run, root, setup, fix"
    );
    assert_eq!(refused("make", gate(&[])), "make.run: must be a command");
    assert_eq!(
        refused("make", gate(&[("run", text("  "))])),
        "make.run: must be a command"
    );
    assert_eq!(
        refused("make", gate(&[run(), ("setup", text("x"))])),
        "make.setup: must be a list of text"
    );
    assert_eq!(
        refused("make", gate(&[run(), ("fix", Setting::Number(1))])),
        "make.fix: must be text"
    );

    for root in ["", "/etc", "docs/", "../up", "a/./b", "a//b"] {
        assert_eq!(
            refused("make", gate(&[run(), ("root", text(root))])),
            "make.root: must be a folder inside the project, such as docs",
            "{root:?}"
        );
    }

    let nested = Listed::from_settings(
        &Settings::default().with("make", gate(&[run(), ("root", text("a/b"))])),
    );
    assert!(nested.is_ok());
}
