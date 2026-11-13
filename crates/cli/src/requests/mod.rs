//! Requests from `atelier finish` and `atelier stop`: immutable files in
//! each project's queue. The worker reads them on its next poll and stays
//! the only writer of `state.json`.

mod file;
mod queue;
pub mod target;

pub use queue::{acknowledge, read, submit};

use std::path::PathBuf;

use crate::paths::Paths;

/// The queue of `repo`'s worker, under atelier's state folder.
pub fn folder(paths: &Paths, repo: &str) -> PathBuf {
    paths.state.join("requests").join(repo)
}
