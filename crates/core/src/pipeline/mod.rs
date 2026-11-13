//! A project's pipelines: steps written in its config, each a kind a plugin
//! offers, checked when the config loads.

mod kinds;
mod read;
mod spec;

use std::fmt;

pub use kinds::Kinds;
pub use spec::{Need, Needs, StepSpec};

/// The most steps a pipeline has, so the machine stays small enough to
/// prove.
pub const MAX_STEPS: usize = 8;

/// The outcome a step settles with when it names none of its own, and the
/// one a list in `after` waits for.
pub const PASSED: &str = "passed";

/// The outcome every step can settle with besides its own.
pub const FAILED: &str = "failed";

/// A checked pipeline: at most eight steps, each of a kind this atelier
/// offers, each waiting only on steps before it, for outcomes they have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pipeline {
    name: String,
    steps: Vec<StepSpec>,
}

/// A pipeline the config describes but atelier cannot run, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineError {
    pub pipeline: String,
    pub problem: String,
}

impl Pipeline {
    /// Its name, which its trigger mark carries, as `atelier:build` does.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its steps, in the order written.
    #[must_use]
    pub fn steps(&self) -> &[StepSpec] {
        &self.steps
    }

    /// The step called `id`.
    #[must_use]
    pub fn step(&self, id: &str) -> Option<&StepSpec> {
        self.steps.iter().find(|step| step.id == id)
    }
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.pipeline, self.problem)
    }
}

impl std::error::Error for PipelineError {}
