//! One poll of a project: what the plugins see, then acting on it.

mod act;
mod observe;
mod project;
mod state;

pub use act::{Move, Report, act};
pub use observe::{Observation, Observed, observe};
pub use project::{Plugins, Project, Source};
pub use state::{Record, State};
