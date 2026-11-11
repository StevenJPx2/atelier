//! The labels that are atelier's marks on GitHub, set per tracker in the
//! project's config, with atelier's own as defaults.

use atelier_core::{Mark, Settings, SettingsError};

/// One label: its name, and the color and description `atelier init`
/// gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub name: String,
    pub color: String,
    pub description: String,
}

/// atelier's labels: one for each mark, and `improvement` for a critic's
/// findings that do not block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Labels {
    pub build: Label,
    pub working: Label,
    pub blocked: Label,
    pub improvement: Label,
}

const KEYS: [&str; 4] = ["build", "working", "blocked", "improvement"];

const FIELDS: [&str; 3] = ["name", "color", "description"];

fn label(name: &str, color: &str, description: &str) -> Label {
    Label {
        name: name.into(),
        color: color.into(),
        description: description.into(),
    }
}

impl Default for Labels {
    fn default() -> Self {
        Self {
            build: label(
                "atelier:build",
                "0e8a16",
                "Start an atelier builder on this issue",
            ),
            working: label(
                "atelier:working",
                "fbca04",
                "An atelier builder holds this issue",
            ),
            blocked: label(
                "atelier:blocked",
                "d93f0b",
                "The atelier builder stopped; the pull request says why",
            ),
            improvement: label(
                "improvement",
                "c5def5",
                "A critic's finding that does not block, kept for later",
            ),
        }
    }
}

impl Labels {
    /// The defaults, with what a tracker's `labels` table sets in their
    /// place:
    ///
    /// ```toml
    /// [[tracker]]
    /// plugin = "github"
    /// labels.working = { name = "bot:working", color = "5319e7" }
    /// ```
    ///
    /// # Errors
    ///
    /// An unknown key, a value that is not text, an empty name, or a color
    /// that is not six hex digits.
    pub fn from_settings(settings: &Settings) -> Result<Self, SettingsError> {
        settings.only("", &["labels"])?;

        let mut labels = Self::default();

        let Some(table) = settings.table("", "labels")? else {
            return Ok(labels);
        };

        table.only("labels", &KEYS)?;

        for (key, label) in KEYS.into_iter().zip(labels.all_mut()) {
            if let Some(fields) = table.table("labels", key)? {
                set(label, fields, &format!("labels.{key}"))?;
            }
        }

        Ok(labels)
    }

    /// The label that is `mark`.
    #[must_use]
    pub const fn mark(&self, mark: Mark) -> &Label {
        match mark {
            Mark::Build => &self.build,
            Mark::Working => &self.working,
            Mark::Blocked => &self.blocked,
        }
    }

    /// Every label, in the order `atelier init` makes them.
    #[must_use]
    pub const fn all(&self) -> [&Label; 4] {
        [&self.build, &self.working, &self.blocked, &self.improvement]
    }

    const fn all_mut(&mut self) -> [&mut Label; 4] {
        [
            &mut self.build,
            &mut self.working,
            &mut self.blocked,
            &mut self.improvement,
        ]
    }
}

fn set(label: &mut Label, fields: &Settings, path: &str) -> Result<(), SettingsError> {
    fields.only(path, &FIELDS)?;

    if let Some(name) = fields.text(path, "name")? {
        if name.trim().is_empty() {
            return Err(SettingsError::new(path, "name", "must not be empty"));
        }

        name.clone_into(&mut label.name);
    }

    if let Some(color) = fields.text(path, "color")? {
        if color.len() != 6 || !color.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(SettingsError::new(
                path,
                "color",
                "must be six hex digits, such as 0e8a16",
            ));
        }

        color.clone_into(&mut label.color);
    }

    if let Some(description) = fields.text(path, "description")? {
        description.clone_into(&mut label.description);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use atelier_core::Setting;

    use super::*;

    fn text(value: &str) -> Setting {
        Setting::Text(value.into())
    }

    fn labels(key: &str, fields: Settings) -> Settings {
        let table = Settings::default().with(key, Setting::Table(fields));

        Settings::default().with("labels", Setting::Table(table))
    }

    #[test]
    fn a_project_renames_or_recolors_a_label_and_keeps_the_rest() {
        let fields = Settings::default()
            .with("name", text("bot:working"))
            .with("color", text("5319E7"));

        let labels = Labels::from_settings(&labels("working", fields)).unwrap();

        assert_eq!(labels.mark(Mark::Working).name, "bot:working");
        assert_eq!(labels.working.color, "5319E7");
        assert_eq!(
            labels.working.description,
            Labels::default().working.description
        );
        assert_eq!(labels.mark(Mark::Build), &Labels::default().build);
        assert_eq!(labels.mark(Mark::Blocked).name, "atelier:blocked");
        assert_eq!(
            Labels::from_settings(&Settings::default()).unwrap(),
            Labels::default()
        );
    }

    #[test]
    fn a_setting_it_cannot_use_is_named() {
        let cases = [
            (
                Settings::default().with("lables", text("x")),
                "lables: unknown; it takes labels",
            ),
            (
                labels("wroking", Settings::default()),
                "labels.wroking: unknown; it takes build, working, blocked, improvement",
            ),
            (
                labels("build", Settings::default().with("colour", text("x"))),
                "labels.build.colour: unknown; it takes name, color, description",
            ),
            (
                labels("build", Settings::default().with("name", text(" "))),
                "labels.build.name: must not be empty",
            ),
            (
                labels("blocked", Settings::default().with("color", text("red"))),
                "labels.blocked.color: must be six hex digits, such as 0e8a16",
            ),
            (
                labels("blocked", Settings::default().with("color", text("12345g"))),
                "labels.blocked.color: must be six hex digits, such as 0e8a16",
            ),
            (
                labels(
                    "improvement",
                    Settings::default().with("description", Setting::Flag(true)),
                ),
                "labels.improvement.description: must be text",
            ),
        ];

        for (settings, message) in cases {
            assert_eq!(
                Labels::from_settings(&settings).unwrap_err().to_string(),
                message
            );
        }
    }
}
