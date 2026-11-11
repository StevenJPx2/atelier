//! A plugin's settings from a project's config, as plain values, so core
//! needs no config format and each plugin reads only its own keys.

use std::collections::BTreeMap;
use std::fmt;

/// One setting's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Setting {
    Text(String),
    Number(i64),
    Flag(bool),
    List(Vec<Self>),
    Table(Settings),
}

/// A table of settings, such as one `[[tracker]]` entry without its
/// `plugin` and `filter`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings(BTreeMap<String, Setting>);

/// A setting a plugin cannot use, named by its dotted path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsError {
    pub key: String,
    pub problem: String,
}

impl Settings {
    #[must_use]
    pub fn with(mut self, key: &str, value: Setting) -> Self {
        self.0.insert(key.to_owned(), value);
        self
    }

    /// Fails on the first key not in `known`, so a misspelt setting is
    /// never quietly ignored.
    ///
    /// # Errors
    ///
    /// A key the plugin does not know, named under `path`.
    pub fn only(&self, path: &str, known: &[&str]) -> Result<(), SettingsError> {
        let unknown = self.0.keys().find(|key| !known.contains(&key.as_str()));
        let problem = format!("unknown; it takes {}", known.join(", "));

        unknown.map_or(Ok(()), |key| Err(SettingsError::new(path, key, &problem)))
    }

    /// The text at `key`, if set.
    ///
    /// # Errors
    ///
    /// The key holds something else.
    pub fn text(&self, path: &str, key: &str) -> Result<Option<&str>, SettingsError> {
        match self.0.get(key) {
            None => Ok(None),
            Some(Setting::Text(text)) => Ok(Some(text)),
            Some(_) => Err(SettingsError::new(path, key, "must be text")),
        }
    }

    /// The table at `key`, if set.
    ///
    /// # Errors
    ///
    /// The key holds something else.
    pub fn table(&self, path: &str, key: &str) -> Result<Option<&Self>, SettingsError> {
        match self.0.get(key) {
            None => Ok(None),
            Some(Setting::Table(table)) => Ok(Some(table)),
            Some(_) => Err(SettingsError::new(path, key, "must be a table")),
        }
    }
}

impl SettingsError {
    /// The error for `key` under `path`, such as `labels.working`.
    #[must_use]
    pub fn new(path: &str, key: &str, problem: &str) -> Self {
        let key = if path.is_empty() {
            key.to_owned()
        } else {
            format!("{path}.{key}")
        };

        Self {
            key,
            problem: problem.to_owned(),
        }
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.key, self.problem)
    }
}

impl std::error::Error for SettingsError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plugin_reads_its_keys_and_names_the_path_of_any_it_cannot_use() {
        let working = Settings::default().with("name", Setting::Text("bot:working".into()));
        let labels = Settings::default()
            .with("working", Setting::Table(working))
            .with("build", Setting::Number(3));
        let settings = Settings::default().with("labels", Setting::Table(labels));

        let labels = settings.table("", "labels").unwrap().unwrap();
        let working = labels.table("labels", "working").unwrap().unwrap();

        assert_eq!(
            working.text("labels.working", "name").unwrap(),
            Some("bot:working")
        );
        assert_eq!(working.text("labels.working", "color").unwrap(), None);
        assert_eq!(settings.table("", "missing").unwrap(), None);
        assert_eq!(
            labels.table("labels", "build").unwrap_err().to_string(),
            "labels.build: must be a table"
        );
        assert_eq!(
            labels.text("labels", "working").unwrap_err().to_string(),
            "labels.working: must be text"
        );
        assert_eq!(
            settings
                .only("", &["filter", "lables"])
                .unwrap_err()
                .to_string(),
            "labels: unknown; it takes filter, lables"
        );
        assert!(labels.only("labels", &["build", "working"]).is_ok());
    }
}
