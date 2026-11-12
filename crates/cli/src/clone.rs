//! atelier's own clone of each watched project. The daemon reads only these,
//! never your checkout, so it needs no access to folders macOS guards.

use atelier_core::Repo;
use atelier_git::Repository;
use atelier_github::GitHub;
use color_eyre::eyre::Result;

use crate::paths::Paths;

/// The project's clone, without making it.
///
/// # Errors
///
/// `repo` is not `owner/name`.
pub fn of(paths: &Paths, repo: &str) -> Result<Repository> {
    Ok(Repository::under(&paths.clones(), &Repo(repo.to_owned()))?)
}

/// The project's clone, made from GitHub when it is missing.
///
/// # Errors
///
/// `repo` is not `owner/name`, or the clone cannot be made.
pub fn ensure(paths: &Paths, repo: &str) -> Result<Repository> {
    let local = of(paths, repo)?;

    if !local.exists() {
        let url = GitHub::new().clone_url(&Repo(repo.to_owned()))?;
        local.create(&url)?;
    }

    Ok(local)
}
