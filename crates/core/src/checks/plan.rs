//! Which gates and fixes a change needs.

use std::collections::{BTreeMap, BTreeSet};

use super::{Checks, Fix, Gate, Root, Tree};

/// What a project sets for one subtree, in `[paths."…"]`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Override {
    /// The checks plugins that run there; every one when none.
    pub checks: Option<Vec<String>>,
    /// Gates and fixes not run there, by full name, such as
    /// `commands/mutants`.
    pub skip: Vec<String>,
}

/// Each subtree's override, by its path. The deepest one holding a path
/// is the one that applies there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides(pub BTreeMap<String, Override>);

/// A gate or a fix of one plugin, in one of its roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned<T> {
    pub plugin: String,
    pub root: Root,
    pub item: T,
}

impl Planned<Gate> {
    /// Its full name, such as `commands/clippy`.
    #[must_use]
    pub fn name(&self) -> String {
        format!("{}/{}", self.plugin, self.item.name)
    }
}

impl Planned<Fix> {
    /// Its full name, such as `commands/clippy`.
    #[must_use]
    pub fn name(&self) -> String {
        format!("{}/{}", self.plugin, self.item.name)
    }
}

/// The gates to run, less what the overrides drop.
///
/// They are those of every root `changed` touches, or of every root when
/// there is no diff. Each changed file touches the deepest roots that hold
/// it. They come in order of root, then plugin, then each plugin's own
/// order.
///
/// ```
/// use atelier_core::{Checks, Gate, Fix, MemoryTree, Overrides, Root, Tree, gates};
///
/// struct Docs;
///
/// impl Checks for Docs {
///     fn roots(&self, _: &dyn Tree) -> Vec<Root> {
///         vec![Root(".".into()), Root("docs".into())]
///     }
///
///     fn gates(&self, _: &dyn Tree, root: &Root) -> Vec<Gate> {
///         let run = format!("lint {}", root.0);
///         vec![Gate { name: "lint".into(), setup: vec![], run }]
///     }
///
///     fn fixes(&self, _: &dyn Tree, _: &Root) -> Vec<Fix> {
///         vec![]
///     }
/// }
///
/// let tree = MemoryTree::new([("README.md", "…"), ("docs/plan.md", "…")]);
/// let changed = ["docs/plan.md".to_owned()];
///
/// let planned = gates(&[("docs", &Docs)], &tree, &Overrides::default(), Some(&changed));
///
/// assert_eq!(planned.len(), 1);
/// assert_eq!(planned[0].root, Root("docs".into()));
/// assert_eq!(planned[0].name(), "docs/lint");
/// ```
#[must_use]
pub fn gates(
    plugins: &[(&str, &dyn Checks)],
    tree: &dyn Tree,
    overrides: &Overrides,
    changed: Option<&[String]>,
) -> Vec<Planned<Gate>> {
    touched(plugins, tree, overrides, changed)
        .into_iter()
        .flat_map(|(plugin, checks, root)| {
            checks
                .gates(tree, &root)
                .into_iter()
                .map(move |item| Planned {
                    plugin: plugin.to_owned(),
                    root: root.clone(),
                    item,
                })
        })
        .filter(|planned| !overrides.skips(&planned.name(), &planned.root.0))
        .collect()
}

/// The fixes for `files`: those of every root the files touch, less what
/// the overrides drop, in the order of [`gates`].
#[must_use]
pub fn fixes(
    plugins: &[(&str, &dyn Checks)],
    tree: &dyn Tree,
    overrides: &Overrides,
    files: &[String],
) -> Vec<Planned<Fix>> {
    touched(plugins, tree, overrides, Some(files))
        .into_iter()
        .flat_map(|(plugin, checks, root)| {
            checks
                .fixes(tree, &root)
                .into_iter()
                .map(move |item| Planned {
                    plugin: plugin.to_owned(),
                    root: root.clone(),
                    item,
                })
        })
        .filter(|planned| !overrides.skips(&planned.name(), &planned.root.0))
        .collect()
}

type Found<'p> = (&'p str, &'p dyn Checks, Root);

/// Every plugin's roots that run where they are, sorted by root, kept when
/// `changed` touches them or there is no diff.
fn touched<'p>(
    plugins: &[(&'p str, &'p dyn Checks)],
    tree: &dyn Tree,
    overrides: &Overrides,
    changed: Option<&[String]>,
) -> Vec<Found<'p>> {
    let mut roots: Vec<Found<'p>> = plugins
        .iter()
        .flat_map(|&(plugin, checks)| {
            checks
                .roots(tree)
                .into_iter()
                .map(move |root| (plugin, checks, root))
        })
        .filter(|(plugin, _, root)| overrides.allows(plugin, &root.0))
        .collect();
    roots.sort_by(|a, b| a.2.cmp(&b.2));

    let Some(changed) = changed else {
        return roots;
    };

    let mut hit = BTreeSet::new();

    for file in changed {
        let holding = || {
            roots.iter().enumerate().filter(|(_, (plugin, _, root))| {
                holds(&root.0, file) && overrides.allows(plugin, file)
            })
        };

        if let Some(deepest) = holding().map(|(_, (_, _, root))| depth(&root.0)).max() {
            hit.extend(
                holding()
                    .filter(|(_, (_, _, root))| depth(&root.0) == deepest)
                    .map(|(index, _)| index),
            );
        }
    }

    roots
        .into_iter()
        .enumerate()
        .filter(|(index, _)| hit.contains(index))
        .map(|(_, found)| found)
        .collect()
}

impl Overrides {
    /// The deepest override holding `path`.
    fn at(&self, path: &str) -> Option<&Override> {
        self.0
            .iter()
            .filter(|(dir, _)| holds(dir, path))
            .max_by_key(|(dir, _)| depth(dir))
            .map(|(_, found)| found)
    }

    fn allows(&self, plugin: &str, path: &str) -> bool {
        self.at(path)
            .and_then(|found| found.checks.as_ref())
            .is_none_or(|checks| checks.iter().any(|name| name == plugin))
    }

    fn skips(&self, name: &str, path: &str) -> bool {
        self.at(path)
            .is_some_and(|found| found.skip.iter().any(|skipped| skipped == name))
    }
}

/// Whether directory `dir` holds `path`, a file or a directory.
fn holds(dir: &str, path: &str) -> bool {
    dir == "."
        || path == dir
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn depth(dir: &str) -> usize {
    if dir == "." {
        0
    } else {
        dir.split('/').count()
    }
}
