//! The pre-commit hook builders' worktrees use: every commit a builder
//! makes is fixed first, and the project's own hook still runs.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr};

/// Writes `pre-commit` into `hooks`, calling `atelier` from `PATH`, or
/// `exe` when it is not there.
///
/// # Errors
///
/// The folder or the file cannot be written.
pub fn write(hooks: &Path, exe: &Path) -> Result<PathBuf> {
    fs::create_dir_all(hooks).wrap_err_with(|| format!("making {}", hooks.display()))?;

    let path = hooks.join("pre-commit");
    let exe = exe.to_string_lossy().replace('\'', "'\\''");
    let script = format!(
        r#"#!/bin/sh
# atelier's pre-commit hook. Only builders' worktrees use it, through their
# own core.hooksPath; the repository and its other worktrees keep theirs.

atelier=$(command -v atelier || printf '%s' '{exe}')
"$atelier" fix --staged ||
    echo "atelier: a fix failed; the commit goes on, and CI's gate says why" >&2

# Then the project's own hook, if it has one.
hooks=$(git config --local --get core.hooksPath) ||
    hooks="$(git rev-parse --git-common-dir)/hooks"
case "$hooks" in
    /*) ;;
    *) hooks="$(git rev-parse --show-toplevel)/$hooks" ;;
esac

if [ -x "$hooks/pre-commit" ]; then
    exec "$hooks/pre-commit" "$@"
fi
"#
    );

    fs::write(&path, script).wrap_err_with(|| format!("writing {}", path.display()))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;

    Ok(path)
}
