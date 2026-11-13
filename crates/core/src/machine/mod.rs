//! The issue machine: one pure `step` from a phase and an event to the next
//! phase and the actions that carry it out.

mod action;
mod limits;
mod phase;
mod step;

#[cfg(kani)]
mod proofs;

pub use action::{Action, Mark};
pub use limits::{Limits, LimitsError};
pub use phase::{Event, Phase, Reason};
pub use step::step;
