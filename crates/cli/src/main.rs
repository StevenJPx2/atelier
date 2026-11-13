//! `atelier`: watch projects, and turn labelled issues into builders.

mod clone;
mod command_step;
mod commands;
mod config;
mod daemon;
mod gates;
mod hook;
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
    /// Watch a project: clone it into atelier's own folder, and add it to
    /// ~/.config/atelier/projects.toml.
    Add {
        /// The repository, as owner/name.
        repo: String,
    },
    /// Poll every watched project; what the Homebrew service runs.
    Daemon,
    /// Each project's settings, and each issue atelier is following.
    Status,
    /// Each root of the checkout and its gates.
    Gates {
        /// Only the roots the changes since this base touch.
        #[arg(long)]
        diff: Option<String>,
        /// As a JSON matrix, `[{"gate": …, "root": …}]`, as CI reads it.
        #[arg(long)]
        json: bool,
        /// A folder in the checkout.
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Run the gates the changes touch, or one gate in one root.
    Check {
        /// Measure changes from this base, not origin's base branch.
        #[arg(long)]
        diff: Option<String>,
        /// The gate to run, such as commands/clippy.
        #[arg(long, requires = "root")]
        gate: Option<String>,
        /// The root to run it in, such as `.`.
        #[arg(long, requires = "gate")]
        root: Option<String>,
        /// Run each gate's setup first.
        #[arg(long)]
        setup: bool,
        /// A folder in the checkout.
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Run the fixes of the roots some files touch.
    Fix {
        /// The staged files, staged again once fixed.
        #[arg(long, conflicts_with = "files")]
        staged: bool,
        /// Files, from the top of the checkout.
        files: Vec<String>,
        /// A folder in the checkout.
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
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
        Command::Add { repo } => commands::add(&paths, &repo)?,
        Command::Daemon => daemon::run(&paths)?,
        Command::Status => commands::status(&paths)?,
        Command::Gates { diff, json, path } => gates::list(&paths, &path, diff.as_deref(), json)?,
        Command::Check {
            diff,
            gate,
            root,
            setup,
            path,
        } => {
            let selection = gates::Selection {
                diff: diff.as_deref(),
                gate: gate.as_deref(),
                root: root.as_deref(),
                setup,
            };
            return gates::check(&paths, &path, &selection);
        }
        Command::Fix {
            staged,
            files,
            path,
        } => return gates::fix(&paths, &path, staged, &files),
    }

    Ok(ExitCode::SUCCESS)
}
