//! `atelier gates`, `check` and `fix`: a project's gates, run in a
//! checkout.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use atelier_core::{Checks, Root, fixes};
use atelier_git::WorkingTree;
use color_eyre::eyre::{Result, bail};

use crate::commands::here;
use crate::config::{self, Resolved};
use crate::plugins::Build;

/// The longest one gate or fix may run, setup included.
const DEADLINE: Duration = Duration::from_hours(1);

/// How many of a failure's last lines are printed.
const TAIL: usize = 40;

/// The checkout holding `path`, and its project's settings, read as the
/// daemon reads them, from `origin`.
fn project(path: &Path) -> Result<(WorkingTree, Resolved)> {
    let (local, entry) = here(path)?;
    let resolved = config::resolve(&entry, &local)?;
    let tree = WorkingTree::read(path)?;

    Ok((tree, resolved))
}

fn refs<'a>(checks: &'a [(&'static str, Box<dyn Checks + 'a>)]) -> Vec<(&'a str, &'a dyn Checks)> {
    checks
        .iter()
        .map(|(name, checks)| (*name, checks.as_ref()))
        .collect()
}

/// `origin/<base>`, or the base the caller gave.
fn base(resolved: &Resolved, diff: Option<&str>) -> String {
    diff.map_or_else(
        || format!("origin/{}", resolved.project.base),
        str::to_owned,
    )
}

/// Prints each root and its gates; with `diff`, those whose root the
/// changes since it touch.
///
/// # Errors
///
/// The checkout, its settings or its diff cannot be read.
pub fn list(path: &Path, diff: Option<&str>, json: bool) -> Result<()> {
    let (tree, resolved) = project(path)?;
    let build = Build::new();
    let checks = build.checks(&resolved)?;
    let changed = diff.map(|base| tree.changed(base)).transpose()?;
    let planned = atelier_core::gates(
        &refs(&checks),
        &tree,
        &resolved.overrides,
        changed.as_deref(),
    );

    if json {
        let matrix: Vec<serde_json::Value> = planned
            .iter()
            .map(|gate| serde_json::json!({ "gate": gate.name(), "root": gate.root.0 }))
            .collect();
        println!("{}", serde_json::to_string(&matrix)?);
        return Ok(());
    }

    let width = widest(planned.iter().map(|gate| &gate.root));

    for gate in &planned {
        println!("{:<width$}  {}", gate.root.0, gate.name());
    }

    if planned.is_empty() {
        println!("no gates");
    }

    Ok(())
}

/// Which gates `check` runs.
pub struct Selection<'a> {
    pub diff: Option<&'a str>,
    pub gate: Option<&'a str>,
    pub root: Option<&'a str>,
    pub setup: bool,
}

/// Runs one named gate in one root, or every gate whose root the changes
/// since the base touch, committed or not.
///
/// # Errors
///
/// The checkout, its settings or its diff cannot be read, or the named
/// gate is not in that root.
pub fn check(path: &Path, selection: &Selection<'_>) -> Result<ExitCode> {
    let (tree, resolved) = project(path)?;
    let build = Build::new();
    let checks = build.checks(&resolved)?;
    let plugins = refs(&checks);
    let base = base(&resolved, selection.diff);

    let planned = match (selection.gate, selection.root) {
        (Some(gate), Some(root)) => {
            let all = atelier_core::gates(&plugins, &tree, &resolved.overrides, None);
            let one: Vec<_> = all
                .into_iter()
                .filter(|planned| planned.name() == gate && planned.root.0 == root)
                .collect();

            if one.is_empty() {
                bail!("no gate {gate} in {root}; atelier gates lists them");
            }

            one
        }
        (None, None) => {
            let changed = tree.changed(&base)?;
            atelier_core::gates(&plugins, &tree, &resolved.overrides, Some(&changed))
        }
        _ => bail!("--gate and --root go together"),
    };

    let rootw = widest(planned.iter().map(|gate| &gate.root));
    let namew = planned
        .iter()
        .map(|gate| gate.name().len())
        .max()
        .unwrap_or(0);
    let mut failed = false;

    for gate in &planned {
        let mut lines: Vec<&str> = Vec::new();

        if selection.setup {
            lines.extend(gate.item.setup.iter().map(String::as_str));
        }

        lines.push(&gate.item.run);
        let ran = run(&folder(&tree, &gate.root), &lines, &base);
        let outcome = if ran.passed { "pass" } else { "fail" };

        println!(
            "{:<rootw$}  {:<namew$}  {outcome}  {:>6.1} s",
            gate.root.0,
            gate.name(),
            ran.took.as_secs_f64()
        );

        if !ran.passed {
            failed = true;
            print_tail(&ran.output);
        }
    }

    if planned.is_empty() {
        println!("no gates for these changes");
    }

    Ok(exit(failed))
}

/// Runs the fixes of the roots `files` touch, or the staged files with
/// `staged`, which are then staged again, except those with unstaged
/// changes too.
///
/// # Errors
///
/// The checkout or its settings cannot be read, or git cannot stage.
pub fn fix(path: &Path, staged: bool, files: &[String]) -> Result<ExitCode> {
    let (tree, resolved) = project(path)?;
    let build = Build::new();
    let checks = build.checks(&resolved)?;
    let base = base(&resolved, None);
    let stage = staged.then(|| tree.staged()).transpose()?;
    let files = stage
        .as_ref()
        .map_or_else(|| files.to_vec(), |stage| stage.files.clone());
    let planned = fixes(&refs(&checks), &tree, &resolved.overrides, &files);
    let rootw = widest(planned.iter().map(|fix| &fix.root));
    let mut failed = false;

    for fix in &planned {
        let ran = run(&folder(&tree, &fix.root), &[&fix.item.run], &base);
        let outcome = if ran.passed { "fixed" } else { "failed" };

        println!("{:<rootw$}  {}  {outcome}", fix.root.0, fix.name());

        if !ran.passed {
            failed = true;
            print_tail(&ran.output);
        }
    }

    if let Some(stage) = stage {
        let whole: Vec<String> = stage
            .files
            .iter()
            .filter(|file| !stage.partly.contains(file))
            .cloned()
            .collect();
        tree.stage(&whole)?;
    }

    Ok(exit(failed))
}

const fn exit(failed: bool) -> ExitCode {
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn widest<'a>(roots: impl Iterator<Item = &'a Root>) -> usize {
    roots.map(|root| root.0.len()).max().unwrap_or(0)
}

fn folder(tree: &WorkingTree, root: &Root) -> PathBuf {
    if root.0 == "." {
        tree.root().to_owned()
    } else {
        tree.root().join(&root.0)
    }
}

struct Ran {
    passed: bool,
    took: Duration,
    output: String,
}

/// Runs each line with `sh -c` in `dir`, stopping at the first failure.
fn run(dir: &Path, lines: &[&str], base: &str) -> Ran {
    let started = Instant::now();
    let mut output = String::new();

    for line in lines {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(line)
            .current_dir(dir)
            .env("ATELIER_BASE", base);

        let passed = match atelier_command::run(command, DEADLINE) {
            Ok(ran) => {
                output.push_str(&ran.stdout);
                output.push_str(&ran.stderr);
                ran.success
            }
            Err(error) => {
                output.push_str(&error.message);
                false
            }
        };

        if !passed {
            return Ran {
                passed,
                took: started.elapsed(),
                output,
            };
        }
    }

    Ran {
        passed: true,
        took: started.elapsed(),
        output,
    }
}

fn print_tail(output: &str) {
    let lines: Vec<&str> = output.lines().collect();
    let skip = lines.len().saturating_sub(TAIL);

    for line in lines.iter().skip(skip) {
        println!("    {line}");
    }
}
