use std::path::PathBuf;

use crate::{Agent, Filter, Forge, Pipeline, PluginError, Repo, Step, Tracker, Worktrees};

/// A watched project, with what its config and atelier found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub repo: Repo,
    pub local: PathBuf,
    pub base: String,
    /// The forge plugin's name, as steps resolve to it.
    pub forge: String,
    /// Its pipelines, each checked against its plugins.
    pub pipelines: Vec<Pipeline>,
    /// Where each run's steps keep what they keep between polls.
    pub steps: PathBuf,
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
    /// Each plugin's kinds of step, by the plugin's name.
    pub steps: Vec<(&'a str, &'a dyn Step)>,
}

impl Project {
    /// The pipeline called `name`.
    #[must_use]
    pub fn pipeline(&self, name: &str) -> Option<&Pipeline> {
        self.pipelines
            .iter()
            .find(|pipeline| pipeline.name() == name)
    }
}

impl<'a> Plugins<'a> {
    pub(crate) fn source(&self, name: &str) -> Option<&Source<'a>> {
        self.trackers.iter().find(|source| source.name == name)
    }

    /// The kinds of step of the plugin called `plugin`.
    pub(crate) fn step(&self, plugin: &str) -> Result<&'a dyn Step, PluginError> {
        self.steps
            .iter()
            .find(|(name, _)| *name == plugin)
            .map(|(_, step)| *step)
            .ok_or_else(|| PluginError::permanent(format!("this atelier has no plugin {plugin}")))
    }
}
