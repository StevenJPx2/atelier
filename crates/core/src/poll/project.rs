use std::path::PathBuf;

use crate::{Agent, Filter, Forge, Pipeline, Repo, Tracker, Worktrees};

/// A watched project, with what its config and atelier found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub repo: Repo,
    pub local: PathBuf,
    pub base: String,
    pub pipeline: Pipeline,
}

/// One of a project's trackers, with its filter.
pub struct Source<'a> {
    pub name: &'a str,
    pub tracker: &'a dyn Tracker,
    pub filter: Filter,
}

/// The plugins a project's polls go through.
pub struct Plugins<'a> {
    pub trackers: Vec<Source<'a>>,
    pub forge: &'a dyn Forge,
    pub agent: &'a dyn Agent,
    pub worktrees: &'a dyn Worktrees,
}

impl<'a> Plugins<'a> {
    pub(crate) fn source(&self, name: &str) -> Option<&Source<'a>> {
        self.trackers.iter().find(|source| source.name == name)
    }
}
