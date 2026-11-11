//! Plugins and the registry each fills with the roles it provides.

use std::fmt;

use crate::{Agent, Forge, Tracker};

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
}

/// Every role the build's plugins fill, by plugin name.
pub struct Registry<'a> {
    naming: &'static str,
    trackers: Vec<(&'static str, &'a dyn Tracker)>,
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
            forges: Vec::new(),
            agents: Vec::new(),
        };

        for plugin in plugins {
            registry.naming = plugin.name();
            plugin.register(&mut registry);
        }

        registry
    }

    pub fn tracker(&mut self, tracker: &'a dyn Tracker) {
        self.trackers.push((self.naming, tracker));
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
    pub fn find_tracker(&self, name: &str) -> Result<&'a dyn Tracker, MissingPlugin> {
        find(&self.trackers, Role::Tracker, name)
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
