//! `atelier init`, `add` and `status`.

use std::path::Path;

use atelier_core::Repo;
use atelier_git::Checkout;
use atelier_github::GitHub;
use color_eyre::eyre::{Result, WrapErr, bail, eyre};

use crate::config::{self, repo_from_url};
use crate::paths::Paths;
use crate::projects::{self, Entry};
use crate::state;

/// The checkout at `path`, and the project its `origin` names.
fn here(path: &Path) -> Result<(Checkout, Entry)> {
    let path = path
        .canonicalize()
        .wrap_err_with(|| format!("{} is not a folder", path.display()))?;
    let checkout = Checkout::new(&path);
    let url = checkout.origin()?;
    let repo = repo_from_url(&url).ok_or_else(|| eyre!("cannot read a repository from {url}"))?;

    Ok((
        checkout,
        Entry {
            repo,
            path,
            filter: std::collections::BTreeMap::default(),
        },
    ))
}

/// Makes each tracker's labels.
///
/// # Errors
///
/// The project's settings are not valid, or its tracker refuses.
pub fn init(_paths: &Paths, path: &Path) -> Result<()> {
    let (checkout, entry) = here(path)?;
    let resolved = config::resolve(&entry, &checkout)?;
    let repo = Repo(entry.repo.clone());

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

/// Watches the project whose checkout is at `path`.
///
/// # Errors
///
/// The checkout's `origin` is another repository, or it is already watched.
pub fn add(paths: &Paths, repo: &str, path: &Path) -> Result<()> {
    let (_, entry) = here(path)?;

    if !entry.repo.eq_ignore_ascii_case(repo) {
        bail!(
            "{} has origin {}, not {repo}",
            entry.path.display(),
            entry.repo
        );
    }

    projects::add(&paths.projects(), repo, &entry.path)?;
    println!("watching {repo} at {}", entry.path.display());

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
        println!("{}  {}", entry.repo, entry.path.display());

        match config::resolve(&entry, &Checkout::new(paths.expand(&entry.path))) {
            Ok(resolved) => {
                for (name, value, origin) in &resolved.shown {
                    println!("  {name:<11} {value:<24} {origin}");
                }
            }
            Err(error) => println!("  settings: {error:#}"),
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
