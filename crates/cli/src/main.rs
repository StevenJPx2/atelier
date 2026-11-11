//! The atelier command-line entry point.

use std::process::ExitCode;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "atelier",
    version,
    about = "A personal software factory",
    arg_required_else_help = true
)]
struct Cli {}

fn main() -> std::io::Result<ExitCode> {
    match Cli::try_parse() {
        Ok(_) => Ok(ExitCode::SUCCESS),
        Err(error) => {
            let code = if error.use_stderr() {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            };
            error.print()?;
            Ok(code)
        }
    }
}
