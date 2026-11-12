//! Plugins and the registry each fills with the roles it provides.

use std::fmt;

use crate::{Agent, Checks, Forge, Settings, SettingsError, Tracker};

/// A crate atelier is built with. Its `register` adds the roles it fills.
pub trait Plugin {
    fn name(&self) -> &'static str;

    fn register<'a>(&'a self, registry: &mut Registry<'a>);
}

/// A role a plugin fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Tracker,
    Forge,
    Agent,
    Checks,
}

/// Where a checks plugin runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum On {
    /// In every project, finding its roots itself, as a language plugin
    /// does.
    Found,
    /// Only in a project that names it in `checks`.
    Named,
}

/// What a tracker plugin registers: its tracker for one project, built
/// from that project's settings for it.
type Make<'a> = dyn Fn(&Settings) -> Result<Box<dyn Tracker + 'a>, SettingsError> + 'a;

/// Builds a plugin's tracker for one project.
pub struct MakeTracker<'a>(Box<Make<'a>>);

impl<'a> MakeTracker<'a> {
    /// # Errors
    ///
    /// A setting the plugin cannot use.
    pub fn make(&self, settings: &Settings) -> Result<Box<dyn Tracker + 'a>, SettingsError> {
        (self.0)(settings)
    }
}

type BuildChecks<'a> = dyn Fn(&Settings) -> Result<Box<dyn Checks + 'a>, SettingsError> + 'a;

/// Builds a checks plugin for one project, from that project's settings
/// for it.
pub struct MakeChecks<'a> {
    on: On,
    build: Box<BuildChecks<'a>>,
}

impl<'a> MakeChecks<'a> {
    #[must_use]
    pub const fn on(&self) -> On {
        self.on
    }

    /// # Errors
    ///
    /// A setting the plugin cannot use.
    pub fn make(&self, settings: &Settings) -> Result<Box<dyn Checks + 'a>, SettingsError> {
        (self.build)(settings)
    }
}

/// Every role the build's plugins fill, by plugin name.
pub struct Registry<'a> {
    naming: &'static str,
    trackers: Vec<(&'static str, MakeTracker<'a>)>,
    checks: Vec<(&'static str, MakeChecks<'a>)>,
    forges: Vec<(&'static str, &'a dyn Forge)>,
    agents: Vec<(&'static str, &'a dyn Agent)>,
}

/// A project named a plugin, or relied on one, the build lacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingPlugin {
    pub role: Role,
    pub name: Option<String>,
    pub available: Vec<&'static str>,
}

impl<'a> Registry<'a> {
    /// The registry of `plugins`, each registering its roles in turn.
    #[must_use]
    pub fn new(plugins: &[&'a dyn Plugin]) -> Self {
        let mut registry = Self {
            naming: "",
            trackers: Vec::new(),
            checks: Vec::new(),
            forges: Vec::new(),
            agents: Vec::new(),
        };

        for plugin in plugins {
            registry.naming = plugin.name();
            plugin.register(&mut registry);
        }

        registry
    }

    /// Registers how to build this plugin's tracker for a project.
    pub fn tracker(
        &mut self,
        make: impl Fn(&Settings) -> Result<Box<dyn Tracker + 'a>, SettingsError> + 'a,
    ) {
        self.trackers
            .push((self.naming, MakeTracker(Box::new(make))));
    }

    /// Registers how to build this checks plugin for a project, and where
    /// it runs.
    pub fn checks(
        &mut self,
        on: On,
        build: impl Fn(&Settings) -> Result<Box<dyn Checks + 'a>, SettingsError> + 'a,
    ) {
        let make = MakeChecks {
            on,
            build: Box::new(build),
        };
        self.checks.push((self.naming, make));
    }

    pub fn forge(&mut self, forge: &'a dyn Forge) {
        self.forges.push((self.naming, forge));
    }

    pub fn agent(&mut self, agent: &'a dyn Agent) {
        self.agents.push((self.naming, agent));
    }

    /// # Errors
    ///
    /// No plugin by that name is a tracker.
    pub fn find_tracker(&self, name: &str) -> Result<&MakeTracker<'a>, MissingPlugin> {
        self.trackers
            .iter()
            .find(|(found, _)| *found == name)
            .map(|(_, make)| make)
            .ok_or_else(|| MissingPlugin {
                role: Role::Tracker,
                name: Some(name.to_owned()),
                available: self.trackers.iter().map(|(name, _)| *name).collect(),
            })
    }

    /// The checks plugins a project runs: every one found in each project,
    /// then each it `named` that is not, in the project's order.
    ///
    /// # Errors
    ///
    /// No checks plugin has a name in `named`.
    pub fn find_checks(
        &self,
        named: &[String],
    ) -> Result<Vec<(&'static str, &MakeChecks<'a>)>, MissingPlugin> {
        let found = self
            .checks
            .iter()
            .filter(|(_, make)| make.on == On::Found)
            .map(|(name, make)| (*name, make));
        let mut runs: Vec<_> = found.collect();

        for wanted in named {
            let (name, make) = self
                .checks
                .iter()
                .find(|(name, _)| name == wanted)
                .ok_or_else(|| MissingPlugin {
                    role: Role::Checks,
                    name: Some(wanted.clone()),
                    available: self.checks.iter().map(|(name, _)| *name).collect(),
                })?;

            if !runs.iter().any(|(running, _)| running == name) {
                runs.push((*name, make));
            }
        }

        Ok(runs)
    }

    /// # Errors
    ///
    /// No plugin by that name is a forge.
    pub fn find_forge(&self, name: &str) -> Result<&'a dyn Forge, MissingPlugin> {
        find(&self.forges, Role::Forge, name)
    }

    /// The agent `name`, or the build's only agent when no name is given.
    ///
    /// # Errors
    ///
    /// No agent by that name, or no name and not exactly one agent.
    pub fn find_agent(&self, name: Option<&str>) -> Result<&'a dyn Agent, MissingPlugin> {
        match (name, self.agents.as_slice()) {
            (Some(name), agents) => find(agents, Role::Agent, name),
            (None, [(_, agent)]) => Ok(*agent),
            (None, agents) => Err(MissingPlugin {
                role: Role::Agent,
                name: None,
                available: agents.iter().map(|(name, _)| *name).collect(),
            }),
        }
    }
}

fn find<'a, T: ?Sized>(
    entries: &[(&'static str, &'a T)],
    role: Role,
    name: &str,
) -> Result<&'a T, MissingPlugin> {
    entries
        .iter()
        .find(|(found, _)| *found == name)
        .map(|(_, plugin)| *plugin)
        .ok_or_else(|| MissingPlugin {
            role,
            name: Some(name.to_owned()),
            available: entries.iter().map(|(name, _)| *name).collect(),
        })
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Tracker => "tracker",
            Self::Forge => "forge",
            Self::Agent => "agent",
            Self::Checks => "checks",
        })
    }
}

impl fmt::Display for MissingPlugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let available = if self.available.is_empty() {
            "none".to_owned()
        } else {
            self.available.join(", ")
        };

        match &self.name {
            Some(name) => write!(
                f,
                "this atelier has no {} plugin \"{name}\"; it has {available}",
                self.role
            ),
            None => write!(
                f,
                "name the {} plugin to use; this atelier has {available}",
                self.role
            ),
        }
    }
}

impl std::error::Error for MissingPlugin {}
