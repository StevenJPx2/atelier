use crate::Settings;

/// One checked step of a pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepSpec {
    /// Its name in the pipeline.
    pub id: String,
    /// The kind as written, such as `forge.checks`.
    pub uses: String,
    /// The plugin whose kind it is, such as `github` for `forge.checks`.
    pub plugin: &'static str,
    /// The kind's name in that plugin, such as `checks`.
    pub kind: String,
    pub needs: Needs,
    /// The outcomes it settles with, besides `failed`.
    pub outcomes: Vec<String>,
    /// How many times it may fail before the failure is final.
    pub rounds: u8,
    /// A push sends the run back to it.
    pub restart: bool,
    /// It stays alive once settled, as an agent's session does.
    pub resident: bool,
    /// The kind's own settings, as its plugin configured them.
    pub settings: Settings,
}

/// What a step waits for: every need in `all`, and, when `any` has
/// needs, one of those.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Needs {
    pub all: Vec<Need>,
    pub any: Vec<Need>,
}

/// An earlier step, and the outcomes of it that satisfy the need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Need {
    pub step: String,
    pub outcomes: Vec<String>,
}

impl StepSpec {
    /// The kind as it resolved, such as `github.checks`.
    #[must_use]
    pub fn resolved(&self) -> String {
        format!("{}.{}", self.plugin, self.kind)
    }
}
