use crate::{Kind, Step};

/// The kinds of step a project can use, by namespace: each plugin's name,
/// and each role the project fills, such as `forge` for its forge.
#[derive(Default)]
pub struct Kinds<'a> {
    namespaces: Vec<Namespace<'a>>,
}

struct Namespace<'a> {
    name: String,
    plugin: &'static str,
    step: &'a dyn Step,
}

impl<'a> Kinds<'a> {
    /// Adds `plugin`'s kinds under `namespace`, its own name or a role's.
    #[must_use]
    pub fn with(mut self, namespace: &str, plugin: &'static str, step: &'a dyn Step) -> Self {
        self.namespaces.push(Namespace {
            name: namespace.to_owned(),
            plugin,
            step,
        });
        self
    }

    /// The plugin under `namespace`, and its kinds.
    pub(crate) fn find(&self, namespace: &str) -> Option<(&'static str, &'a dyn Step)> {
        self.namespaces
            .iter()
            .find(|found| found.name == namespace)
            .map(|found| (found.plugin, found.step))
    }

    /// Every namespace, in order of name.
    pub(crate) fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .namespaces
            .iter()
            .map(|found| found.name.as_str())
            .collect();
        names.sort_unstable();
        names
    }

    /// The kind called `name` in `step`.
    pub(crate) fn kind(step: &dyn Step, name: &str) -> Option<Kind> {
        step.kinds().into_iter().find(|kind| kind.name == name)
    }
}
