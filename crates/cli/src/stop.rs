//! `atelier stop`: a person leaves a stop request for a working run.

use atelier_core::{Command, Request};
use color_eyre::eyre::Result;

use crate::paths::Paths;
use crate::requests;

/// Leaves a stop request for the working run called `selector`, such as
/// `security-audit github#40`. Only the worker changes the state.
///
/// # Errors
///
/// No run or several match, or the request queue cannot be written.
pub fn run(paths: &Paths, selector: &str, repo: Option<&str>) -> Result<()> {
    let target = requests::target::by_key(paths, selector, repo)?;
    let request = Request {
        generation: target.record.generation,
        command: Command::Stop,
    };

    requests::submit(
        &requests::folder(paths, &target.repo),
        &target.key,
        &request,
    )?;
    println!("requested stop: {} {selector}", target.repo);
    Ok(())
}
