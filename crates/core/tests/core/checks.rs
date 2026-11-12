//! Which gates and fixes a change needs, with plugins whose roots and gates
//! are fixed.

use std::collections::BTreeMap;

use atelier_core::{
    Checks, Fix, Gate, MemoryTree, On, Override, Overrides, Planned, Plugin, Registry, Root,
    Settings, Tree, fixes, gates,
};

struct Fixed {
    roots: &'static [&'static str],
    gates: &'static [&'static str],
}

impl Checks for Fixed {
    fn roots(&self, _: &dyn Tree) -> Vec<Root> {
        self.roots.iter().map(|root| Root((*root).into())).collect()
    }

    fn gates(&self, _: &dyn Tree, root: &Root) -> Vec<Gate> {
        self.gates
            .iter()
            .map(|name| Gate {
                name: (*name).into(),
                setup: vec![format!("setup {name}")],
                run: format!("{name} {}", root.0),
            })
            .collect()
    }

    fn fixes(&self, _: &dyn Tree, root: &Root) -> Vec<Fix> {
        self.gates
            .iter()
            .map(|name| Fix {
                name: (*name).into(),
                run: format!("fix {name} {}", root.0),
            })
            .collect()
    }
}

const RUST: Fixed = Fixed {
    roots: &[".", "tools/codegen"],
    gates: &["fmt", "mutants"],
};

const DOCS: Fixed = Fixed {
    roots: &["docs/api", "docs", "vendor"],
    gates: &["lint"],
};

fn tree() -> MemoryTree {
    MemoryTree::new([("Cargo.toml", "[workspace]"), ("docs/README.md", "# Docs")])
}

fn plugins() -> [(&'static str, &'static dyn Checks); 2] {
    [("rust", &RUST), ("docs", &DOCS)]
}

fn changed(files: &[&str]) -> Vec<String> {
    files.iter().map(|file| (*file).to_owned()).collect()
}

fn named(planned: &[Planned<Gate>]) -> Vec<String> {
    planned
        .iter()
        .map(|gate| format!("{} {}", gate.root.0, gate.name()))
        .collect()
}

/// A path, the plugins that run there if set, and the gates it skips.
type Entry<'a> = (&'a str, Option<&'a [&'a str]>, &'a [&'a str]);

fn overrides(entries: &[Entry<'_>]) -> Overrides {
    let strings = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect();

    Overrides(
        entries
            .iter()
            .map(|(path, checks, skip)| {
                let found = Override {
                    checks: checks.map(strings),
                    skip: strings(skip),
                };
                ((*path).to_owned(), found)
            })
            .collect::<BTreeMap<_, _>>(),
    )
}

#[test]
fn a_file_touches_the_deepest_roots_that_hold_it() {
    let none = Overrides::default();
    let plan = |files: &[&str]| named(&gates(&plugins(), &tree(), &none, Some(&changed(files))));

    assert_eq!(plan(&["docs/api/index.md"]), ["docs/api docs/lint"]);
    assert_eq!(plan(&["docs/guide.md"]), ["docs docs/lint"]);
    assert_eq!(plan(&["docsite/x.md"]), [". rust/fmt", ". rust/mutants"]);
    assert_eq!(
        plan(&["src/lib.rs", "docs/guide.md"]),
        [". rust/fmt", ". rust/mutants", "docs docs/lint"]
    );
    assert_eq!(plan(&[]), Vec::<String>::new());
}

#[test]
fn with_no_diff_every_root_runs_in_order_of_root_then_plugin() {
    let both = Fixed {
        roots: &["."],
        gates: &["build"],
    };
    let plugins: [(&str, &dyn Checks); 3] = [("rust", &RUST), ("docs", &DOCS), ("both", &both)];

    let planned = gates(&plugins, &tree(), &Overrides::default(), None);

    assert_eq!(
        named(&planned),
        [
            ". rust/fmt",
            ". rust/mutants",
            ". both/build",
            "docs docs/lint",
            "docs/api docs/lint",
            "tools/codegen rust/fmt",
            "tools/codegen rust/mutants",
            "vendor docs/lint",
        ]
    );
    assert_eq!(planned[0].item.run, "fmt .");
    assert_eq!(planned[0].item.setup, ["setup fmt"]);
}

#[test]
fn an_override_drops_plugins_and_gates_in_its_subtree_and_the_deepest_wins() {
    let set = overrides(&[
        ("vendor", Some(&[]), &[]),
        ("docs", Some(&["rust"]), &[]),
        (".", None, &["rust/mutants"]),
        ("tools/codegen", None, &[]),
    ]);

    let all = named(&gates(&plugins(), &tree(), &set, None));
    let vendored = named(&gates(
        &plugins(),
        &tree(),
        &set,
        Some(&changed(&["vendor/x.rs"])),
    ));
    let documented = named(&gates(
        &plugins(),
        &tree(),
        &set,
        Some(&changed(&["docs/a.md"])),
    ));

    assert_eq!(
        all,
        [
            ". rust/fmt",
            "tools/codegen rust/fmt",
            "tools/codegen rust/mutants",
        ]
    );
    assert_eq!(vendored, Vec::<String>::new());
    assert_eq!(documented, [". rust/fmt"]);
}

#[test]
fn fixes_follow_the_files_and_the_overrides() {
    let set = overrides(&[("docs", None, &["docs/lint"]), ("docs/api", None, &[])]);
    let files = changed(&["src/lib.rs", "docs/a.md", "docs/api/b.md"]);

    let planned = fixes(&plugins(), &tree(), &set, &files);

    let runs: Vec<String> = planned
        .iter()
        .map(|fix| format!("{} {}", fix.name(), fix.item.run))
        .collect();
    assert_eq!(
        runs,
        [
            "rust/fmt fix fmt .",
            "rust/mutants fix mutants .",
            "docs/lint fix lint docs/api",
        ]
    );
}

/// Finds a root beside each `Cargo.toml`, and builds what its manifest
/// says.
struct Manifests;

impl Checks for Manifests {
    fn roots(&self, tree: &dyn Tree) -> Vec<Root> {
        tree.files()
            .iter()
            .filter_map(|file| file.strip_suffix("Cargo.toml"))
            .map(|dir| {
                dir.strip_suffix('/')
                    .map_or_else(|| Root(".".to_owned()), |dir| Root(dir.to_owned()))
            })
            .collect()
    }

    fn gates(&self, tree: &dyn Tree, root: &Root) -> Vec<Gate> {
        let manifest = if root.0 == "." {
            "Cargo.toml".to_owned()
        } else {
            format!("{}/Cargo.toml", root.0)
        };

        tree.read(&manifest)
            .map(|text| Gate {
                name: "build".into(),
                setup: vec![],
                run: format!("build {}", text.trim()),
            })
            .into_iter()
            .collect()
    }

    fn fixes(&self, _: &dyn Tree, _: &Root) -> Vec<Fix> {
        vec![]
    }
}

#[test]
fn a_plugin_finds_its_roots_and_reads_its_files_in_the_tree() {
    let tree = MemoryTree::new([
        ("Cargo.toml", "[workspace]"),
        ("docs/README.md", "# Docs"),
        ("tools/gen/Cargo.toml", "[package]\n"),
    ]);

    let planned = gates(&[("rust", &Manifests)], &tree, &Overrides::default(), None);

    let runs: Vec<String> = planned
        .iter()
        .map(|gate| format!("{} {}", gate.root.0, gate.item.run))
        .collect();
    assert_eq!(runs, [". build [workspace]", "tools/gen build [package]"]);
    assert_eq!(tree.read("docs/missing.md"), None);
}

struct Language;
struct Named;

impl Plugin for Language {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn register<'a>(&'a self, registry: &mut Registry<'a>) {
        registry.checks(On::Found, |_| Ok(Box::new(RUST)));
    }
}

impl Plugin for Named {
    fn name(&self) -> &'static str {
        "commands"
    }

    fn register<'a>(&'a self, registry: &mut Registry<'a>) {
        registry.checks(On::Named, |_| Ok(Box::new(DOCS)));
    }
}

#[test]
fn found_plugins_always_run_and_named_ones_only_when_named() {
    let registry = Registry::new(&[&Language, &Named]);
    let names = |wanted: &[&str]| {
        let wanted = changed(wanted);
        registry
            .find_checks(&wanted)
            .map(|runs| runs.iter().map(|(name, _)| *name).collect::<Vec<_>>())
    };

    let made = registry.find_checks(&changed(&["commands"])).unwrap()[1]
        .1
        .make(&Settings::default())
        .unwrap();

    assert_eq!(names(&[]).unwrap(), ["rust"]);
    assert_eq!(names(&["commands", "rust"]).unwrap(), ["rust", "commands"]);
    assert_eq!(made.roots(&tree()).len(), 3);
    assert_eq!(
        names(&["docs"]).unwrap_err().to_string(),
        "this atelier has no checks plugin \"docs\"; it has rust, commands"
    );
}
