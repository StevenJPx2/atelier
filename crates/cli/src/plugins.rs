//! The plugins this atelier is built with, and each project's choice of
//! them.

use atelier_core::{Agent, Filter, Forge, Plugin, Plugins, Registry, Source, Tracker};
use atelier_git::Git;
use atelier_github::GitHub;
use atelier_opencode::OpenCode;
use color_eyre::eyre::{Result, bail, eyre};

use crate::config::Resolved;

/// One instance of each plugin crate in the build.
pub struct Build {
    github: GitHub,
    opencode: OpenCode,
}

/// A project's plugins, built from its settings.
pub struct Built<'a> {
    trackers: Vec<(String, Box<dyn Tracker + 'a>, Filter)>,
    forge: &'a dyn Forge,
    agent: &'a dyn Agent,
}

impl Build {
    pub fn new() -> Self {
        Self {
            github: GitHub::new(),
            opencode: OpenCode::new(),
        }
    }

    /// The project's trackers, forge and agent, by the names it resolved.
    ///
    /// # Errors
    ///
    /// A plugin the build lacks, two trackers from one plugin, or a tracker
    /// setting its plugin cannot use.
    pub fn plugins(&self, resolved: &Resolved) -> Result<Built<'_>> {
        let list: [&dyn Plugin; 2] = [&self.github, &self.opencode];
        let registry = Registry::new(&list);
        let mut trackers = Vec::new();

        for spec in &resolved.trackers {
            if trackers.iter().any(|(name, _, _)| name == &spec.plugin) {
                bail!(
                    "two trackers use the {} plugin; a project has one of each",
                    spec.plugin
                );
            }

            let make = registry.find_tracker(&spec.plugin)?;
            let tracker = make
                .make(&spec.settings)
                .map_err(|error| eyre!("the {} tracker: {error}", spec.plugin))?;

            trackers.push((spec.plugin.clone(), tracker, spec.filter.clone()));
        }

        Ok(Built {
            trackers,
            forge: registry.find_forge(&resolved.forge)?,
            agent: registry.find_agent(resolved.agent.as_deref())?,
        })
    }
}

impl Built<'_> {
    /// The trackers' tracker for `name`, such as for `atelier init`.
    pub fn plugins<'b>(&'b self, git: &'b Git) -> Plugins<'b> {
        Plugins {
            trackers: self
                .trackers
                .iter()
                .map(|(name, tracker, filter)| Source {
                    name,
                    tracker: tracker.as_ref(),
                    filter: filter.clone(),
                })
                .collect(),
            forge: self.forge,
            agent: self.agent,
            worktrees: git,
        }
    }
}
