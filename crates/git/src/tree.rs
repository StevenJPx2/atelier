//! A checkout's files, as checks plugins read them, and what changed in it.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use atelier_core::{PluginError, Tree};

use crate::command::checked;

/// A checkout's working tree: its files as git lists them, tracked or
/// untracked and not ignored, read once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingTree {
    root: PathBuf,
    files: Vec<String>,
}

/// The staged files, and those of them that also have unstaged changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Staged {
    pub files: Vec<String>,
    pub partly: Vec<String>,
}

impl WorkingTree {
    /// The working tree of the checkout holding `folder`, from its top.
    ///
    /// # Errors
    ///
    /// The folder is in no checkout.
    pub fn read(folder: &Path) -> Result<Self, PluginError> {
        let top = checked(folder, &["rev-parse", "--show-toplevel"])?;
        let root = PathBuf::from(top.trim());
        let listed = checked(
            &root,
            &[
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ],
        )?;
        let files = names(&listed).into_iter().collect();

        Ok(Self { root, files })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every file changed since `base`: committed since the commit it
    /// shares with `HEAD`, staged, unstaged or untracked, in order.
    ///
    /// # Errors
    ///
    /// `base` is not a commit in this repository.
    pub fn changed(&self, base: &str) -> Result<Vec<String>, PluginError> {
        let since = format!("{base}...HEAD");
        let mut changed = names(&checked(
            &self.root,
            &["diff", "--name-only", "-z", &since],
        )?);

        changed.extend(names(&checked(
            &self.root,
            &["diff", "--name-only", "-z", "HEAD"],
        )?));
        changed.extend(names(&checked(
            &self.root,
            &["ls-files", "-z", "--others", "--exclude-standard"],
        )?));

        Ok(changed.into_iter().collect())
    }

    /// The files staged for the next commit.
    ///
    /// # Errors
    ///
    /// Git cannot run here.
    pub fn staged(&self) -> Result<Staged, PluginError> {
        let staged = names(&checked(
            &self.root,
            &["diff", "--cached", "--name-only", "-z", "--diff-filter=d"],
        )?);
        let unstaged = names(&checked(&self.root, &["diff", "--name-only", "-z"])?);

        Ok(Staged {
            partly: staged.intersection(&unstaged).cloned().collect(),
            files: staged.into_iter().collect(),
        })
    }

    /// Stages `files` as they are now.
    ///
    /// # Errors
    ///
    /// Git refuses a file.
    pub fn stage(&self, files: &[String]) -> Result<(), PluginError> {
        if files.is_empty() {
            return Ok(());
        }

        let mut args = vec!["add", "--"];
        args.extend(files.iter().map(String::as_str));
        checked(&self.root, &args)?;

        Ok(())
    }
}

impl Tree for WorkingTree {
    fn files(&self) -> Vec<String> {
        self.files.clone()
    }

    /// The file's text, for a path inside the tree only.
    fn read(&self, path: &str) -> Option<String> {
        let inside = Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)));

        inside
            .then(|| fs::read_to_string(self.root.join(path)).ok())
            .flatten()
    }
}

/// The paths in git's `-z` output.
fn names(output: &str) -> BTreeSet<String> {
    output
        .split('\0')
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}
