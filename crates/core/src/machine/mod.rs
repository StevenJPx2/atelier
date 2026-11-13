//! The run machine: one pure `step` from a run and an event to the next
//! run and the actions that carry it out, over rungs' step logic.

mod action;
mod phase;
mod step;

#[cfg(kani)]
mod proofs;

pub use action::{Action, Mark};
pub use phase::{Event, Phase, Reason, Run};
pub use step::step;
