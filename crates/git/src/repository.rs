//! What atelier reads of a project's local repository and its `origin`.

use std::fs;
use std::path::{Path, PathBuf};

use atelier_core::{PluginError, Repo, Tree};

use crate::command::{checked, git};
use crate::folder;

/// A project's local repository, bare or a checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    path: PathBuf,
}

impl Repository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// atelier's own clone of `repo`, at `<root>/<owner>/<name>.git`,
    /// whether or not it is made yet.
    ///
    /// # Errors
    ///
    /// The repository is not `owner/name`, or a part could leave the root.
    pub fn under(root: &Path, repo: &Repo) -> Result<Self, PluginError> {
        let (owner, name) = folder::owner_and_name(repo)?;

        Ok(Self::new(root.join(owner).join(format!("{name}.git"))))
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether a bare repository is here.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.path.join("HEAD").is_file()
    }

    /// Makes a bare repository here whose `origin` is `url`, or keeps the
    /// one already here with that `origin`. It fetches nothing: each
    /// branch is fetched when needed, into `origin/<branch>`, and its only
    /// local branches are the issues'.
    ///
    /// # Errors
    ///
    /// The folder cannot be made, or holds a repository with another
    /// `origin`.
    pub fn create(&self, url: &str) -> Result<(), PluginError> {
        if !self.exists() {
            let parent = self.path.parent().unwrap_or(&self.path);
            fs::create_dir_all(parent).map_err(|error| {
                PluginError::permanent(format!("making {}: {error}", parent.display()))
            })?;
            let target = self.path.to_string_lossy();
            checked(parent, &["init", "--quiet", "--bare", &target])?;
        }

        let origin = git(&self.path, &["config", "--get", "remote.origin.url"])?;

        if !origin.success {
            checked(&self.path, &["remote", "add", "origin", url])?;
        } else if origin.stdout.trim() != url {
            return Err(PluginError::permanent(format!(
                "{} has origin {}, not {url}",
                self.path.display(),
                origin.stdout.trim()
            )));
        }

        Ok(())
    }

    /// The URL of `origin` as configured, before git's own `insteadOf`
    /// rewriting, so it names the forge the user chose.
    ///
    /// # Errors
    ///
    /// The folder is not a repository, or has no `origin`.
    pub fn origin(&self) -> Result<String, PluginError> {
        let url = checked(&self.path, &["config", "--get", "remote.origin.url"])?;

        Ok(url.trim().to_owned())
    }

    /// The branch `origin`'s `HEAD` points at, asked of `origin` itself.
    ///
    /// # Errors
    ///
    /// `origin` cannot be reached, or names no default branch.
    pub fn default_branch(&self) -> Result<String, PluginError> {
        let heads = checked(&self.path, &["ls-remote", "--symref", "origin", "HEAD"])?;

        heads
            .lines()
            .find_map(|line| line.strip_prefix("ref: refs/heads/")?.split_once('\t'))
            .map(|(branch, _)| branch.to_owned())
            .ok_or_else(|| PluginError::permanent("origin names no default branch"))
    }

    /// Fetches `branch` from `origin`, so `origin/<branch>` is current.
    ///
    /// # Errors
    ///
    /// `origin` cannot be reached, or has no such branch.
    pub fn fetch(&self, branch: &str) -> Result<(), PluginError> {
        checked(&self.path, &["fetch", "--quiet", "origin", branch])?;
        Ok(())
    }

    /// The file at `path` on `origin/<branch>`, as last fetched; none when
    /// the branch has no such file.
    ///
    /// # Errors
    ///
    /// Git cannot run, or `origin/<branch>` is unknown.
    pub fn show(&self, branch: &str, path: &str) -> Result<Option<String>, PluginError> {
        let remote = format!("origin/{branch}");
        checked(&self.path, &["rev-parse", "--verify", "--quiet", &remote])?;

        let output = git(&self.path, &["show", &format!("{remote}:{path}")])?;

        Ok(output.success.then_some(output.stdout))
    }

    /// `origin/<branch>`'s files, as last fetched, as checks plugins and
    /// step kinds read them.
    ///
    /// # Errors
    ///
    /// Git cannot run, or `origin/<branch>` is unknown.
    pub fn tree(&self, branch: &str) -> Result<BranchTree, PluginError> {
        let remote = format!("origin/{branch}");
        let listed = checked(&self.path, &["ls-tree", "-r", "-z", "--name-only", &remote])?;
        let files = listed
            .split('\0')
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect();

        Ok(BranchTree {
            repository: self.clone(),
            branch: branch.to_owned(),
            files,
        })
    }
}

/// A branch of `origin`'s files, listed once and each read when asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchTree {
    repository: Repository,
    branch: String,
    files: Vec<String>,
}

impl Tree for BranchTree {
    fn files(&self) -> Vec<String> {
        self.files.clone()
    }

    /// The file's text, for a file the branch has.
    fn read(&self, path: &str) -> Option<String> {
        if !self.files.iter().any(|file| file == path) {
            return None;
        }

        self.repository.show(&self.branch, path).ok().flatten()
    }
}
