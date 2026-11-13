//! One poll of a project: what the plugins see, then acting on it.

mod act;
mod observe;
mod project;
mod request;
mod state;

pub use act::{Move, Report, act};
pub use observe::{Observation, Watched, observe};
pub use project::{Plugins, Project, Source};
pub use request::{Command, Request};
pub use state::{Record, State, StepRecord};
