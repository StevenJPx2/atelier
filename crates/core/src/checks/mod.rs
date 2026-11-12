//! The checks role: what a plugin finds to check in a project's tree, and
//! which of it a change needs.

mod plan;
mod tree;

pub use plan::{Override, Overrides, Planned, fixes, gates};
pub use tree::{MemoryTree, Tree};

/// A directory a plugin's gates run in, from the top of the tree: `.` for
/// the top, or a path such as `docs`, without a trailing `/`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Root(pub String);

/// One gate: its setup, such as installing a tool, and the shell command
/// that passes or fails, each run in its root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate {
    pub name: String,
    pub setup: Vec<String>,
    pub run: String,
}

/// A shell command that fixes, in its root, what the gate of the same name
/// fails on, such as a formatter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub name: String,
    pub run: String,
}

/// A plugin that finds what to check in a project. It reads the tree and
/// returns data; it runs nothing.
pub trait Checks {
    /// The directories its gates run in.
    fn roots(&self, tree: &dyn Tree) -> Vec<Root>;

    /// The gates of one of its roots, in the order they run.
    fn gates(&self, tree: &dyn Tree, root: &Root) -> Vec<Gate>;

    /// The fixes of one of its roots, in the order they run.
    fn fixes(&self, tree: &dyn Tree, root: &Root) -> Vec<Fix>;
}
