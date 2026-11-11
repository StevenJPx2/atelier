//! The issue machine: one pure `step` from a phase and an event to the next
//! phase and the actions that carry it out.

mod action;
mod phase;
mod pipeline;
mod step;

#[cfg(kani)]
mod proofs;

pub use action::{Action, Mark};
pub use phase::{Event, Phase, Reason};
pub use pipeline::{Pipeline, PipelineError};
pub use step::step;
