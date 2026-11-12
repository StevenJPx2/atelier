use std::collections::BTreeMap;

/// A project's files, as a checks plugin reads them.
pub trait Tree {
    /// Every file's path from the top, with `/` between its parts, in
    /// order.
    fn files(&self) -> Vec<String>;

    /// The file's text; none when it is missing or not text.
    fn read(&self, path: &str) -> Option<String>;
}

/// A tree held in memory, such as a plugin's tests use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryTree(BTreeMap<String, String>);

impl MemoryTree {
    #[must_use]
    pub fn new<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        Self(
            files
                .into_iter()
                .map(|(path, text)| (path.to_owned(), text.to_owned()))
                .collect(),
        )
    }
}

impl Tree for MemoryTree {
    fn files(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }

    fn read(&self, path: &str) -> Option<String> {
        self.0.get(path).cloned()
    }
}
