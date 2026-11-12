//! The plugins this atelier is built with, and each project's choice of
//! them.

use atelier_commands::Commands;
use atelier_core::{Agent, Checks, Filter, Forge, Plugin, Plugins, Registry, Source, Tracker};
use atelier_git::Git;
use atelier_github::GitHub;
use atelier_opencode::OpenCode;
use color_eyre::eyre::{Result, bail, eyre};

use crate::config::{CONFIG, Resolved};

/// One instance of each plugin crate in the build.
pub struct Build {
    commands: Commands,
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
            commands: Commands,
            github: GitHub::new(),
            opencode: OpenCode::new(),
        }
    }

    fn registry(&self) -> Registry<'_> {
        let list: [&dyn Plugin; 3] = [&self.commands, &self.github, &self.opencode];
        Registry::new(&list)
    }

    /// The project's checks plugins, each built from its own table, by
    /// name: every one found in each project, then each it names.
    ///
    /// # Errors
    ///
    /// A plugin the build lacks, or a setting its plugin cannot use.
    pub fn checks(&self, resolved: &Resolved) -> Result<Vec<(&'static str, Box<dyn Checks + '_>)>> {
        let named: Vec<String> = resolved
            .checks
            .iter()
            .map(|spec| spec.plugin.clone())
            .collect();
        let registry = self.registry();
        let mut built = Vec::new();

        for (name, make) in registry.find_checks(&named)? {
            let settings = resolved
                .checks
                .iter()
                .find(|spec| spec.plugin == name)
                .map(|spec| spec.settings.clone())
                .unwrap_or_default();
            let checks = make
                .make(&settings)
                .map_err(|error| eyre!("{CONFIG}: {name}.{error}"))?;

            built.push((name, checks));
        }

        Ok(built)
    }

    /// The project's trackers, forge and agent, by the names it resolved.
    ///
    /// # Errors
    ///
    /// A plugin the build lacks, two trackers from one plugin, or a tracker
    /// setting its plugin cannot use.
    pub fn plugins(&self, resolved: &Resolved) -> Result<Built<'_>> {
        let registry = self.registry();
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
