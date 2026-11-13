//! `OpenCode` as atelier's agent: each builder is an ordinary session,
//! started through `opencode api` with its brief and permission rules.

mod api;
mod permissions;
mod session;
mod steps;

use std::path::PathBuf;

use atelier_core::{Plugin, Registry};

pub use permissions::{Effect, Rule, rules};

/// The `opencode` plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCode {
    program: PathBuf,
}

impl OpenCode {
    /// `OpenCode` through the `opencode` on `PATH`, and its running server.
    #[must_use]
    pub fn new() -> Self {
        Self::with_program("opencode")
    }

    /// `OpenCode` through another program, such as a stand-in in tests.
    #[must_use]
    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

impl Default for OpenCode {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for OpenCode {
    fn name(&self) -> &'static str {
        "opencode"
    }

    fn register<'a>(&'a self, registry: &mut Registry<'a>) {
        registry.agent(self);
        registry.step(self);
    }
}
