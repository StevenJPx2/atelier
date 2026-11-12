//! `~/.config/atelier/projects.toml`: the projects you watch.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr, bail};
use serde::Deserialize;

/// One watched project.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub repo: String,
    /// Your checkout, which atelier read before it kept its own clone; no
    /// longer read.
    pub path: Option<PathBuf>,
    /// Your own filter for each tracker, by plugin name.
    #[serde(default)]
    pub filter: BTreeMap<String, Filter>,
}

/// A filter as written in either file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub assignee: Option<String>,
    pub author: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    pub query: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    project: Vec<Entry>,
}

/// Every watched project; none when the file does not exist yet.
///
/// # Errors
///
/// The file cannot be read or is not valid.
pub fn read(path: &Path) -> Result<Vec<Entry>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).wrap_err_with(|| format!("reading {}", path.display())),
    };

    let file: File =
        toml::from_str(&text).wrap_err_with(|| format!("reading {}", path.display()))?;

    Ok(file.project)
}

/// Adds a project at the end of the file, leaving the rest as written.
///
/// # Errors
///
/// The project is already watched, or the file cannot be written.
pub fn add(path: &Path, repo: &str) -> Result<()> {
    if read(path)?.iter().any(|entry| entry.repo == repo) {
        bail!("{repo} is already watched; see {}", path.display());
    }

    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }

    // A JSON string is a valid TOML basic string, escapes included.
    let repo = serde_json::to_string(repo)?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;

    write!(file, "\n[[project]]\nrepo = {repo}\n")?;

    Ok(())
}

impl Filter {
    /// Both filters, every field of each applying. Two different people
    /// for one field cannot both match, so that is refused.
    ///
    /// # Errors
    ///
    /// The files name different assignees or authors.
    pub fn and(&self, yours: &Self) -> Result<atelier_core::Filter> {
        let one = |field: &str, project: &Option<String>, yours: &Option<String>| match (
            project, yours,
        ) {
            (Some(project), Some(yours)) if project != yours => {
                bail!(
                    "the project's {field} is {project} and yours is {yours}; no issue matches both"
                )
            }
            (project, yours) => Ok(yours.clone().or_else(|| project.clone())),
        };

        let query = [&self.query, &yours.query]
            .into_iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        let mut labels = self.labels.clone();
        labels.extend(
            yours
                .labels
                .iter()
                .filter(|label| !self.labels.contains(label))
                .cloned(),
        );

        Ok(atelier_core::Filter {
            assignee: one("assignee", &self.assignee, &yours.assignee)?,
            author: one("author", &self.author, &yours.author)?,
            labels,
            query: (!query.is_empty()).then(|| query.join(" ")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(assignee: Option<&str>, labels: &[&str], query: Option<&str>) -> Filter {
        Filter {
            assignee: assignee.map(str::to_owned),
            author: None,
            labels: labels.iter().map(|label| (*label).to_owned()).collect(),
            query: query.map(str::to_owned),
        }
    }

    #[test]
    fn both_filters_apply_and_conflicting_people_are_refused() {
        let project = filter(None, &["backend", "api"], Some("milestone:v1"));
        let yours = filter(Some("me"), &["api", "urgent"], Some("no:milestone"));

        let both = project.and(&yours).unwrap();
        let same = filter(Some("me"), &[], None)
            .and(&filter(Some("me"), &[], None))
            .unwrap();
        let clash = filter(Some("me"), &[], None)
            .and(&filter(Some("you"), &[], None))
            .unwrap_err();
        let none = Filter::default().and(&Filter::default()).unwrap();

        assert_eq!(both.assignee.as_deref(), Some("me"));
        assert_eq!(both.labels, ["backend", "api", "urgent"]);
        assert_eq!(both.query.as_deref(), Some("milestone:v1 no:milestone"));
        assert_eq!(same.assignee.as_deref(), Some("me"));
        assert_eq!(
            clash.to_string(),
            "the project's assignee is me and yours is you; no issue matches both"
        );
        assert_eq!(none.query, None);
    }

    #[test]
    fn a_missing_list_is_empty_and_an_unreadable_one_fails() {
        let folder = std::env::temp_dir().join(format!("atelier-projects-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();

        let missing = read(&folder.join("projects.toml"));
        let unreadable = read(&folder);
        let _ = fs::remove_dir_all(&folder);

        assert_eq!(missing.unwrap(), []);
        assert!(unreadable.is_err());
    }
}
