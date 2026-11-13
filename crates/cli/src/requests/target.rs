use std::path::Path;

use atelier_core::{IssueKey, Record};
use atelier_git::WorkingTree;
use color_eyre::eyre::{Result, WrapErr, bail};

use crate::paths::Paths;
use crate::state;

/// The run a command addresses, read without changing the daemon's file.
pub struct Target {
    pub repo: String,
    pub key: IssueKey,
    pub record: Record,
}

/// The working run whose worktree contains `path`.
pub fn in_worktree(paths: &Paths, path: &Path) -> Result<Target> {
    let top = WorkingTree::read(path)?;
    let root = top.root().canonicalize()?;
    let mut found = Vec::new();

    for (repo, state) in state::load(&paths.state_file())? {
        for (key, record) in state.runs {
            let same = record.worktree.as_ref().is_some_and(|worktree| {
                worktree
                    .canonicalize()
                    .is_ok_and(|worktree| worktree == root)
            });

            if record.phase.is_working() && same {
                found.push(Target {
                    repo: repo.clone(),
                    key,
                    record,
                });
            }
        }
    }

    one(found, "no working run owns this worktree")
}

/// The working run named by `selector`, such as `security-audit github#40`,
/// with `repo` when two projects have the same key.
pub fn by_key(paths: &Paths, selector: &str, repo: Option<&str>) -> Result<Target> {
    let mut found = Vec::new();

    for (name, state) in state::load(&paths.state_file())? {
        if repo.is_some_and(|repo| repo != name) {
            continue;
        }

        for (key, record) in state.runs {
            if record.phase.is_working() && format!("{} {key}", record.pipeline) == selector {
                found.push(Target {
                    repo: name.clone(),
                    key,
                    record,
                });
            }
        }
    }

    one(found, "no working run has that key").wrap_err_with(|| format!("finding {selector}"))
}

fn one(mut found: Vec<Target>, missing: &str) -> Result<Target> {
    if found.len() > 1 {
        let names: Vec<&str> = found.iter().map(|run| run.repo.as_str()).collect();
        bail!(
            "several projects match: {}; use --repo owner/name",
            names.join(", ")
        );
    }

    found
        .pop()
        .ok_or_else(|| color_eyre::eyre::eyre!("{missing}"))
}
