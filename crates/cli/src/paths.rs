//! Where atelier keeps your projects, its state and the worktrees.

use std::env;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, eyre};

/// atelier's folders, from the XDG variables or under your home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config: PathBuf,
    pub state: PathBuf,
    pub data: PathBuf,
    home: PathBuf,
}

impl Paths {
    /// `~/.config/atelier`, `~/.local/state/atelier` and
    /// `~/.local/share/atelier`, unless `XDG_CONFIG_HOME`, `XDG_STATE_HOME`
    /// or `XDG_DATA_HOME` say otherwise.
    ///
    /// # Errors
    ///
    /// `HOME` is not set.
    pub fn from_env() -> Result<Self> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| eyre!("HOME is not set"))?;
        let under = |variable: &str, fallback: &str| {
            env::var_os(variable)
                .filter(|value| !value.is_empty())
                .map_or_else(|| home.join(fallback), PathBuf::from)
                .join("atelier")
        };

        Ok(Self {
            config: under("XDG_CONFIG_HOME", ".config"),
            state: under("XDG_STATE_HOME", ".local/state"),
            data: under("XDG_DATA_HOME", ".local/share"),
            home: home.clone(),
        })
    }

    pub fn projects(&self) -> PathBuf {
        self.config.join("projects.toml")
    }

    pub fn state_file(&self) -> PathBuf {
        self.state.join("state.json")
    }

    pub fn lock(&self) -> PathBuf {
        self.state.join("daemon.lock")
    }

    pub fn worktrees(&self) -> PathBuf {
        self.data.join("worktrees")
    }

    /// `path` with a leading `~` as your home.
    pub fn expand(&self, path: &Path) -> PathBuf {
        path.strip_prefix("~")
            .map_or_else(|_| path.to_owned(), |rest| self.home.join(rest))
    }
}
