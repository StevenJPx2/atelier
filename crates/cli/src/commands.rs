//! `atelier init`, `add` and `status`.

use std::path::Path;

use atelier_core::{CiFile, Repo};
use atelier_git::{Repository, WorkingTree};
use atelier_github::GitHub;
use color_eyre::eyre::{Result, WrapErr, bail, eyre};

use crate::clone;
use crate::config::{self, repo_from_url};
use crate::paths::Paths;
use crate::plugins::Build;
use crate::projects::{self, Entry};
use crate::state;

/// The checkout at `path`, and the project its `origin` names.
pub fn here(path: &Path) -> Result<(Repository, Entry)> {
    let path = path
        .canonicalize()
        .wrap_err_with(|| format!("{} is not a folder", path.display()))?;
    let local = Repository::new(&path);
    let url = local.origin()?;
    let repo = repo_from_url(&url).ok_or_else(|| eyre!("cannot read a repository from {url}"))?;

    Ok((
        local,
        Entry {
            repo,
            path: None,
            filter: std::collections::BTreeMap::default(),
        },
    ))
}

/// The commit a release build was made from; none in any other build.
const RELEASE_COMMIT: Option<&str> = option_env!("ATELIER_COMMIT");

/// Makes each tracker's labels, and writes the forge's CI for the gates
/// when this atelier is a release.
///
/// # Errors
///
/// The project's settings are not valid, its tracker refuses, or a CI file
/// cannot be written.
pub fn init(_paths: &Paths, path: &Path) -> Result<()> {
    let (local, entry) = here(path)?;
    let resolved = config::resolve(&entry, &local)?;
    let repo = Repo(entry.repo.clone());
    let build = Build::new();
    let forge = build.forge(&resolved.forge)?;

    match RELEASE_COMMIT {
        Some(commit) => {
            let release = env!("CARGO_PKG_VERSION");
            let files = forge.ci(release, commit, &resolved.project.base);
            let top = WorkingTree::read(path)?;

            for written in write_ci(top.root(), &files)? {
                println!("{}: wrote {written}", entry.repo);
            }
        }
        None => eprintln!(
            "{}: this atelier is no release build, so it writes no CI",
            entry.repo
        ),
    }

    for spec in &resolved.trackers {
        if spec.plugin != "github" {
            bail!(
                "this atelier has no tracker plugin \"{}\"; it has github",
                spec.plugin
            );
        }

        let tracker = GitHub::new()
            .tracker(&spec.settings)
            .map_err(|error| eyre!("the github tracker: {error}"))?;
        tracker.create_labels(&repo)?;

        let names: Vec<&str> = tracker
            .labels()
            .all()
            .iter()
            .map(|label| label.name.as_str())
            .collect();
        println!("{}: labels {}", entry.repo, names.join(", "));
    }

    Ok(())
}

/// Writes each CI file under `top`, returning their paths.
fn write_ci(top: &Path, files: &[CiFile]) -> Result<Vec<String>> {
    files
        .iter()
        .map(|file| {
            let path = top.join(&file.path);

            if let Some(folder) = path.parent() {
                std::fs::create_dir_all(folder)?;
            }

            std::fs::write(&path, &file.text)
                .wrap_err_with(|| format!("writing {}", path.display()))?;
            Ok(file.path.clone())
        })
        .collect()
}

/// Watches `repo`, making atelier's own clone of it. A clone made for a
/// repository it cannot reach is removed again.
///
/// # Errors
///
/// `repo` is not `owner/name`, cannot be reached, or is already watched.
pub fn add(paths: &Paths, repo: &str) -> Result<()> {
    let fresh = !clone::of(paths, repo)?.exists();
    let local = clone::ensure(paths, repo)?;

    if let Err(error) = local.default_branch() {
        if fresh {
            let _ = std::fs::remove_dir_all(local.path());
        }
        return Err(error).wrap_err_with(|| format!("cannot reach {repo}"));
    }

    projects::add(&paths.projects(), repo)?;
    println!("watching {repo}, cloned in {}", local.path().display());

    Ok(())
}

/// Prints each project's settings, where each came from, and each issue
/// with its pull request, from the state alone: it asks no forge.
///
/// # Errors
///
/// `projects.toml` or `state.json` cannot be read.
pub fn status(paths: &Paths) -> Result<()> {
    let states = state::load(&paths.state_file())?;

    for entry in projects::read(&paths.projects())? {
        let local = clone::of(paths, &entry.repo)?;
        println!("{}  {}", entry.repo, local.path().display());

        if local.exists() {
            let resolved = config::resolve(&entry, &local)
                .and_then(|resolved| Build::new().checks(&resolved).map(|_| resolved));

            match resolved {
                Ok(resolved) => {
                    for (name, value, origin) in &resolved.shown {
                        println!("  {name:<11} {value:<24} {origin}");
                    }
                }
                Err(error) => println!("  settings: {error:#}"),
            }
        } else {
            println!("  settings: not cloned yet; the daemon clones it");
        }

        let Some(state) = states.get(&entry.repo) else {
            continue;
        };

        for (key, record) in &state.issues {
            let key = key.to_string();
            let phase = state::shown(record.phase);
            let session = record
                .session
                .as_ref()
                .map_or("-", |session| session.id.as_str());
            let pull_request = record
                .pull_request
                .map_or_else(|| "-".to_owned(), |number| format!("#{number}"));

            println!(
                "  {key:<11} {phase:<24} {}  {session}  {pull_request}",
                record.branch
            );
        }
    }

    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_ci_file_is_written_in_its_folder_under_the_top() {
        let top = std::env::temp_dir().join(format!("atelier-ci-{}", std::process::id()));
        let files = [CiFile {
            path: ".github/workflows/atelier.yml".into(),
            text: "name: atelier\n".into(),
        }];

        let written = write_ci(&top, &files).unwrap();
        let text = std::fs::read_to_string(top.join(".github/workflows/atelier.yml"));
        let _ = std::fs::remove_dir_all(&top);

        assert_eq!(written, [".github/workflows/atelier.yml"]);
        assert_eq!(text.unwrap(), "name: atelier\n");
    }
}
