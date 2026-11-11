//! GitHub as a tracker, for issues, and a forge, for pull requests and
//! their checks, through `gh` and the user's own sign-in.

mod api;
mod forge;
mod labels;
mod tracker;

use std::path::PathBuf;

use atelier_core::{Plugin, Registry};

pub use labels::{Label, Labels};
pub use tracker::GitHubTracker;

/// The `github` plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHub {
    program: PathBuf,
}

impl GitHub {
    /// GitHub through the `gh` on `PATH`.
    #[must_use]
    pub fn new() -> Self {
        Self::with_program("gh")
    }

    /// GitHub through another `gh`, such as a stand-in in tests.
    #[must_use]
    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

impl Default for GitHub {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for GitHub {
    fn name(&self) -> &'static str {
        "github"
    }

    fn register<'a>(&'a self, registry: &mut Registry<'a>) {
        registry.tracker(|settings| Ok(Box::new(self.tracker(settings)?)));
        registry.forge(self);
    }
}
