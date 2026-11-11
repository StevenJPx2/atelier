//! `atelier`: watch projects, and turn labelled issues into builders.

mod commands;
mod config;
mod daemon;
mod log;
mod paths;
mod plugins;
mod projects;
mod state;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use color_eyre::eyre::Result;

use crate::paths::Paths;

#[derive(Debug, Parser)]
#[command(name = "atelier", version, about = "A personal software factory")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Make the project's labels on its tracker.
    Init {
        /// The project's checkout.
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Watch a project: add it to ~/.config/atelier/projects.toml.
    Add {
        /// The repository, as owner/name.
        repo: String,
        /// The project's checkout.
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Poll every watched project; what the Homebrew service runs.
    Daemon,
    /// Each project's settings, and each issue atelier is following.
    Status,
}

fn main() -> Result<ExitCode> {
    color_eyre::install()?;

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            error.print()?;
            return Ok(if error.use_stderr() {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            });
        }
    };

    let paths = Paths::from_env()?;

    match cli.command {
        Command::Init { path } => commands::init(&paths, &path)?,
        Command::Add { repo, path } => commands::add(&paths, &repo, &path)?,
        Command::Daemon => daemon::run(&paths)?,
        Command::Status => commands::status(&paths)?,
    }

    Ok(ExitCode::SUCCESS)
}
