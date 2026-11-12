//! A project's settings: what atelier finds, and what `.atelier/config.toml`
//! sets instead, read from `origin` so a branch cannot change its limits.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use atelier_core::{Pipeline, Project, Repo, Setting, Settings};
use atelier_git::Repository;
use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::Deserialize;

use crate::projects::{Entry, Filter};

const CONFIG: &str = ".atelier/config.toml";

/// Polling faster than this would only spend rate limit.
const MIN_POLL_SECS: u64 = 15;

const DEFAULT_POLL_SECS: u64 = 30;

const DEFAULT_RED_ROUNDS: u8 = 5;

/// `.atelier/config.toml`, as written: only what differs.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    base: Option<String>,
    #[serde(default)]
    tracker: Vec<toml::Table>,
    forge: Option<String>,
    agent: Option<String>,
    #[serde(default)]
    checks: Vec<String>,
    #[serde(default)]
    stages: Vec<String>,
    #[serde(default)]
    limits: BTreeMap<String, u8>,
    poll: Option<u64>,
}

/// Where a setting came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Found,
    Set,
}

/// One of a project's trackers, ready to build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerSpec {
    pub plugin: String,
    pub filter: atelier_core::Filter,
    pub settings: Settings,
}

/// Everything a project's polls run with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub project: Project,
    pub trackers: Vec<TrackerSpec>,
    pub forge: String,
    pub agent: Option<String>,
    pub poll: Duration,
    /// Each setting, its value and where it came from, for `atelier status`.
    pub shown: Vec<(&'static str, String, Origin)>,
}

/// The project's settings: found from its local repository and `origin`, with the
/// config read from `origin`'s default branch for `base`, and from
/// `origin/<base>` for everything else.
///
/// # Errors
///
/// `origin` cannot be reached or is not a forge atelier knows, the config is
/// not valid, or it names a step no plugin in this build provides.
pub fn resolve(entry: &Entry, local: &Repository) -> Result<Resolved> {
    let url = local.origin()?;
    let platform = platform(&url)?;
    let found = repo_from_url(&url).ok_or_else(|| eyre!("cannot read a repository from {url}"))?;

    if !found.eq_ignore_ascii_case(&entry.repo) {
        bail!(
            "{} has origin {found}, not {}",
            local.path().display(),
            entry.repo
        );
    }

    let default = local.default_branch()?;
    local.fetch(&default)?;
    let first = parse(local.show(&default, CONFIG)?.as_deref(), &default)?;

    let (base, file) = match first.base.clone() {
        Some(base) if base != default => {
            local.fetch(&base)?;
            (
                base.clone(),
                parse(local.show(&base, CONFIG)?.as_deref(), &base)?,
            )
        }
        Some(base) => (base, first.clone()),
        None => (default, first.clone()),
    };

    let origin = |set: bool| if set { Origin::Set } else { Origin::Found };
    let mut resolved = build(entry, local, platform, base, file)?;
    resolved.shown.insert(
        0,
        (
            "base",
            resolved.project.base.clone(),
            origin(first.base.is_some()),
        ),
    );

    Ok(resolved)
}

fn build(
    entry: &Entry,
    local: &Repository,
    platform: &str,
    base: String,
    file: File,
) -> Result<Resolved> {
    refuse_unbuilt("checks", &file.checks)?;
    refuse_unbuilt("stage", &file.stages)?;

    let mut limits = file.limits.clone();
    let red_rounds = limits.remove("red_rounds");

    if let Some(key) = limits.keys().next() {
        bail!("{CONFIG}: limits.{key} is unknown; it takes red_rounds");
    }

    let pipeline = Pipeline::new(red_rounds.unwrap_or(DEFAULT_RED_ROUNDS), [])
        .map_err(|error| eyre!("{CONFIG}: {error}"))?;

    let poll = file.poll.unwrap_or(DEFAULT_POLL_SECS);

    if poll < MIN_POLL_SECS {
        bail!("{CONFIG}: poll is {poll}; it must be at least {MIN_POLL_SECS} seconds");
    }

    let trackers = trackers(entry, platform, &file.tracker)?;
    let forge = file.forge.clone().unwrap_or_else(|| platform.to_owned());
    let set = |value: bool| if value { Origin::Set } else { Origin::Found };
    let names = trackers
        .iter()
        .map(|tracker| tracker.plugin.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let shown = vec![
        ("trackers", names, set(!file.tracker.is_empty())),
        ("forge", forge.clone(), set(file.forge.is_some())),
        (
            "agent",
            file.agent.clone().unwrap_or_else(|| "the only one".into()),
            set(file.agent.is_some()),
        ),
        (
            "red_rounds",
            pipeline.red_rounds().to_string(),
            set(red_rounds.is_some()),
        ),
        ("poll", format!("{poll} s"), set(file.poll.is_some())),
    ];

    Ok(Resolved {
        project: Project {
            repo: Repo(entry.repo.clone()),
            local: local.path().to_owned(),
            base,
            pipeline,
        },
        trackers,
        forge,
        agent: file.agent,
        poll: Duration::from_secs(poll),
        shown,
    })
}

fn parse(text: Option<&str>, branch: &str) -> Result<File> {
    text.map_or_else(
        || Ok(File::default()),
        |text| toml::from_str(text).wrap_err_with(|| format!("{CONFIG} on origin/{branch}")),
    )
}

/// The build has no checks or stage plugins yet, so naming one fails as a
/// missing plugin does.
fn refuse_unbuilt(role: &str, names: &[String]) -> Result<()> {
    match names.first() {
        Some(name) => bail!("this atelier has no {role} plugin \"{name}\"; it has none"),
        None => Ok(()),
    }
}

fn trackers(entry: &Entry, platform: &str, tables: &[toml::Table]) -> Result<Vec<TrackerSpec>> {
    let detected = [toml::Table::new()];
    let tables = if tables.is_empty() {
        &detected[..]
    } else {
        tables
    };

    tables
        .iter()
        .map(|table| {
            let mut table = table.clone();
            let plugin = match table.remove("plugin") {
                Some(toml::Value::String(plugin)) => plugin,
                Some(_) => bail!("{CONFIG}: a tracker's plugin must be text"),
                None => platform.to_owned(),
            };
            let project: Filter = match table.remove("filter") {
                Some(filter) => filter
                    .try_into()
                    .wrap_err_with(|| format!("{CONFIG}: the {plugin} tracker's filter"))?,
                None => Filter::default(),
            };
            let yours = entry.filter.get(&plugin).cloned().unwrap_or_default();
            let filter = project.and(&yours)?;
            let settings =
                settings(table).wrap_err_with(|| format!("{CONFIG}: the {plugin} tracker"))?;

            Ok(TrackerSpec {
                plugin,
                filter,
                settings,
            })
        })
        .collect()
}

/// A TOML table as plugin settings.
///
/// # Errors
///
/// A float or a date, which no plugin takes.
pub fn settings(table: toml::Table) -> Result<Settings> {
    table
        .into_iter()
        .try_fold(Settings::default(), |settings, (key, value)| {
            Ok(settings.with(&key, setting(value).wrap_err_with(|| key.clone())?))
        })
}

fn setting(value: toml::Value) -> Result<Setting> {
    Ok(match value {
        toml::Value::String(text) => Setting::Text(text),
        toml::Value::Integer(number) => Setting::Number(number),
        toml::Value::Boolean(flag) => Setting::Flag(flag),
        toml::Value::Array(values) => {
            Setting::List(values.into_iter().map(setting).collect::<Result<_>>()?)
        }
        toml::Value::Table(table) => Setting::Table(settings(table)?),
        toml::Value::Float(_) | toml::Value::Datetime(_) => {
            bail!("must be text, a number, a flag, a list or a table")
        }
    })
}

/// The tracker and forge a remote means.
///
/// # Errors
///
/// A host atelier has no plugin for.
pub fn platform(url: &str) -> Result<&'static str> {
    if url.contains("github.com") {
        Ok("github")
    } else {
        bail!("atelier has no tracker or forge for {url}")
    }
}

/// `owner/name` from a GitHub remote, over HTTPS or SSH.
pub fn repo_from_url(url: &str) -> Option<String> {
    let rest = [
        "https://github.com/",
        "git@github.com:",
        "ssh://git@github.com/",
    ]
    .iter()
    .find_map(|prefix| url.strip_prefix(prefix))?;
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let (owner, name) = rest.split_once('/')?;

    (!owner.is_empty() && !name.is_empty() && !name.contains('/'))
        .then(|| format!("{owner}/{name}"))
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Found => "found",
            Self::Set => "set",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_github_remote_names_its_repository_over_https_or_ssh() {
        let cases = [
            (
                "https://github.com/StevenJPx2/chauffeur.git",
                Some("StevenJPx2/chauffeur"),
            ),
            (
                "https://github.com/StevenJPx2/chauffeur/",
                Some("StevenJPx2/chauffeur"),
            ),
            (
                "git@github.com:StevenJPx2/chauffeur.git",
                Some("StevenJPx2/chauffeur"),
            ),
            (
                "ssh://git@github.com/StevenJPx2/chauffeur",
                Some("StevenJPx2/chauffeur"),
            ),
            ("https://github.com/StevenJPx2", None),
            ("https://github.com//chauffeur", None),
            ("https://github.com/StevenJPx2/", None),
            ("https://github.com/a/b/c", None),
            ("https://gitlab.com/StevenJPx2/chauffeur", None),
        ];

        for (url, repo) in cases {
            assert_eq!(repo_from_url(url).as_deref(), repo, "{url}");
        }

        assert_eq!(platform("git@github.com:a/b").unwrap(), "github");
        assert!(platform("https://gitlab.com/a/b").is_err());
        assert_eq!(
            [Origin::Found.to_string(), Origin::Set.to_string()],
            ["found", "set"]
        );
    }
}
