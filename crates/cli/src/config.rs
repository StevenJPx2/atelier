//! A project's settings: what atelier finds, and what `.atelier/config.toml`
//! sets instead, read from `origin` so a branch cannot change its pipelines.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use atelier_core::{Override, Overrides, Project, Repo, Setting, Settings};
use atelier_git::Repository;
use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::Deserialize;

use crate::projects::{Entry, Filter};

pub const CONFIG: &str = ".atelier/config.toml";

/// Polling faster than this would only spend rate limit.
const MIN_POLL_SECS: u64 = 15;

const DEFAULT_POLL_SECS: u64 = 30;

/// `.atelier/config.toml`, as written: only what differs.
#[derive(Debug, Clone, Default, Deserialize)]
struct File {
    base: Option<String>,
    #[serde(default)]
    tracker: Vec<toml::Table>,
    forge: Option<String>,
    agent: Option<String>,
    #[serde(default)]
    checks: Vec<String>,
    /// Refused: a stage is a step of `[pipeline.build]`.
    stages: Option<toml::Value>,
    /// Refused: a limit is a step's `rounds`.
    limits: Option<toml::Value>,
    poll: Option<u64>,
    #[serde(default)]
    paths: BTreeMap<String, PathFile>,
    #[serde(default)]
    pipeline: BTreeMap<String, PipelineFile>,
    /// Every other key: each must be a named checks plugin's own table,
    /// such as `[commands]`.
    #[serde(flatten)]
    plugins: BTreeMap<String, toml::Value>,
}

/// One `[paths."…"]` entry, as written.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathFile {
    checks: Option<Vec<String>>,
    #[serde(default)]
    skip: Vec<String>,
}

/// One `[pipeline.<name>]`, as written: its steps, in order.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PipelineFile {
    #[serde(default)]
    step: Vec<toml::Table>,
}

/// The built-in `build` pipeline, written in the same terms.
const BUILD: &str = include_str!("build.toml");

/// A pipeline as the config writes it, checked against the project's
/// plugins once they are known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub name: String,
    pub steps: Vec<Settings>,
    pub origin: Origin,
}

/// One of a project's named checks plugins, with its own table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckSpec {
    pub plugin: String,
    pub settings: Settings,
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
    pub checks: Vec<CheckSpec>,
    pub overrides: Overrides,
    /// The built-in `build`, unless the config writes its own, then each
    /// pipeline the config writes, by name.
    pub pipelines: Vec<Written>,
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
    if file.stages.is_some() {
        bail!("{CONFIG}: stages is unknown; a stage is a step of [pipeline.build]");
    }

    if file.limits.is_some() {
        bail!(
            "{CONFIG}: limits is unknown; set rounds on a step of [pipeline.build], such as checks"
        );
    }

    let checks = checks(&file)?;
    let overrides = overrides(&file.paths)?;
    let pipelines = pipelines(&file.pipeline)?;

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
        ("poll", format!("{poll} s"), set(file.poll.is_some())),
        (
            "checks",
            if file.checks.is_empty() {
                "none".to_owned()
            } else {
                file.checks.join(", ")
            },
            set(!file.checks.is_empty()),
        ),
    ];

    Ok(Resolved {
        project: Project {
            repo: Repo(entry.repo.clone()),
            local: local.path().to_owned(),
            base,
            forge: forge.clone(),
            pipelines: Vec::new(),
            steps: std::path::PathBuf::new(),
        },
        trackers,
        checks,
        overrides,
        pipelines,
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

/// Each named checks plugin with its own table. Any other key in the file
/// is refused, so a misspelt setting is never quietly ignored.
fn checks(file: &File) -> Result<Vec<CheckSpec>> {
    if let Some(key) = file.plugins.keys().find(|key| !file.checks.contains(key)) {
        bail!("{CONFIG}: {key} is unknown; a checks plugin's table needs it in checks");
    }

    file.checks
        .iter()
        .map(|plugin| {
            let table = match file.plugins.get(plugin) {
                Some(toml::Value::Table(table)) => table.clone(),
                Some(_) => bail!("{CONFIG}: {plugin} must be a table"),
                None => toml::Table::new(),
            };
            let settings = settings(table).wrap_err_with(|| format!("{CONFIG}: {plugin}"))?;

            Ok(CheckSpec {
                plugin: plugin.clone(),
                settings,
            })
        })
        .collect()
}

/// The built-in `build` first, unless the config writes `[pipeline.build]`,
/// then each pipeline the config writes, by name.
fn pipelines(written: &BTreeMap<String, PipelineFile>) -> Result<Vec<Written>> {
    let builtin: File = toml::from_str(BUILD).wrap_err("the built-in build pipeline")?;
    let mut all: Vec<Written> = Vec::new();

    let ours = builtin
        .pipeline
        .into_iter()
        .filter(|(name, _)| !written.contains_key(name))
        .map(|entry| (entry, Origin::Found));
    let theirs = written
        .iter()
        .map(|(name, file)| ((name.clone(), file.clone()), Origin::Set));

    for ((name, file), origin) in ours.chain(theirs) {
        let steps = file
            .step
            .into_iter()
            .map(|table| settings(table).wrap_err_with(|| format!("{CONFIG}: pipeline.{name}")))
            .collect::<Result<_>>()?;

        all.push(Written {
            name,
            steps,
            origin,
        });
    }

    Ok(all)
}

/// `[paths."…"]`, each a folder inside the project, or `.` for all of it.
fn overrides(paths: &BTreeMap<String, PathFile>) -> Result<Overrides> {
    let folder = |path: &str| {
        path == "."
            || path
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
    };

    paths
        .iter()
        .map(|(path, entry)| {
            if !folder(path) {
                bail!("{CONFIG}: paths.\"{path}\" must be a folder inside the project");
            }

            let found = Override {
                checks: entry.checks.clone(),
                skip: entry.skip.clone(),
            };
            Ok((path.clone(), found))
        })
        .collect::<Result<_>>()
        .map(Overrides)
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
    fn an_override_is_for_a_folder_inside_the_project() {
        let one = |path: &str| {
            let entry = PathFile {
                checks: Some(vec!["commands".into()]),
                skip: vec!["commands/mutants".into()],
            };
            overrides(&BTreeMap::from([(path.to_owned(), entry)]))
        };

        for path in [".", "docs", "tools/codegen"] {
            let found = one(path).unwrap();
            let entry = &found.0[path];

            assert_eq!(entry.checks.as_deref(), Some(&["commands".to_owned()][..]));
            assert_eq!(entry.skip, ["commands/mutants"]);
        }

        for path in ["", "..", "../up", "/etc", "docs/", "a/./b", "a//b", "a/.."] {
            let refusal = one(path).unwrap_err().to_string();

            assert!(
                refusal.contains("must be a folder inside the project"),
                "{path:?}"
            );
        }
    }

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
