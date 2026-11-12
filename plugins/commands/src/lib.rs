//! The `commands` checks plugin: a project lists its gates as shell
//! commands in `.atelier/config.toml`, each with its root, setup and fix.
//!
//! ```
//! use atelier_commands::Listed;
//! use atelier_core::{Checks, MemoryTree, Root, Setting, Settings};
//!
//! let clippy = Settings::default()
//!     .with("run", Setting::Text("cargo clippy -- -D warnings".into()))
//!     .with("fix", Setting::Text("cargo clippy --fix --allow-dirty".into()));
//! let listed = Listed::from_settings(&Settings::default().with("clippy", Setting::Table(clippy)))?;
//! let tree = MemoryTree::new([("Cargo.toml", "[workspace]")]);
//!
//! let roots = listed.roots(&tree);
//! let gates = listed.gates(&tree, &roots[0]);
//!
//! assert_eq!(roots, [Root(".".into())]);
//! assert_eq!(gates[0].name, "clippy");
//! assert_eq!(gates[0].run, "cargo clippy -- -D warnings");
//! # Ok::<(), atelier_core::SettingsError>(())
//! ```

use atelier_core::{
    Checks, Fix, Gate, On, Plugin, Registry, Root, Setting, Settings, SettingsError, Tree,
};

/// The `commands` plugin. It finds nothing by itself, so it runs only in a
/// project that names it in `checks`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Commands;

impl Plugin for Commands {
    fn name(&self) -> &'static str {
        "commands"
    }

    fn register<'a>(&'a self, registry: &mut Registry<'a>) {
        registry.checks(On::Named, |settings| {
            Ok(Box::new(Listed::from_settings(settings)?))
        });
    }
}

/// One project's gates, as its `[commands.<name>]` tables list them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    commands: Vec<Command>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Command {
    name: String,
    root: Root,
    setup: Vec<String>,
    run: String,
    fix: Option<String>,
}

impl Listed {
    /// The gates the settings list, in order of name. Each takes `run`,
    /// and optionally `root` (`.` when unset), `setup` and `fix`.
    ///
    /// # Errors
    ///
    /// A gate that is not a table, a name or root that cannot be used, an
    /// unknown or mistyped key, or a `run` with no command.
    pub fn from_settings(settings: &Settings) -> Result<Self, SettingsError> {
        let commands = settings
            .iter()
            .map(|(name, value)| command(name, value))
            .collect::<Result<_, _>>()?;

        Ok(Self { commands })
    }
}

fn command(name: &str, value: &Setting) -> Result<Command, SettingsError> {
    let Setting::Table(table) = value else {
        return Err(SettingsError::new("", name, "must be a table with run"));
    };
    let named = |c: char| c.is_ascii_alphanumeric() || "-_".contains(c);

    if !name.chars().all(named) {
        return Err(SettingsError::new(
            "",
            name,
            "a gate's name takes letters, digits, - and _",
        ));
    }

    table.only(name, &["run", "root", "setup", "fix"])?;

    let run = table
        .text(name, "run")?
        .filter(|run| !run.trim().is_empty())
        .ok_or_else(|| SettingsError::new(name, "run", "must be a command"))?;
    let root = table.text(name, "root")?.unwrap_or(".");

    Ok(Command {
        name: name.to_owned(),
        root: inside(name, root)?,
        setup: table
            .texts(name, "setup")?
            .into_iter()
            .map(str::to_owned)
            .collect(),
        run: run.to_owned(),
        fix: table.text(name, "fix")?.map(str::to_owned),
    })
}

/// `path` as a root, when it names a folder inside the project.
fn inside(name: &str, path: &str) -> Result<Root, SettingsError> {
    let part = |part: &str| !part.is_empty() && part != "." && part != "..";

    if path == "." || path.split('/').all(part) {
        Ok(Root(path.to_owned()))
    } else {
        Err(SettingsError::new(
            name,
            "root",
            "must be a folder inside the project, such as docs",
        ))
    }
}

impl Checks for Listed {
    /// Each gate's root once, in order of the gates' names, leaving out a
    /// folder with no files.
    fn roots(&self, tree: &dyn Tree) -> Vec<Root> {
        let files = tree.files();
        let holds = |root: &Root| {
            let prefix = format!("{}/", root.0);
            root.0 == "." || files.iter().any(|file| file.starts_with(&prefix))
        };
        let mut roots: Vec<Root> = Vec::new();

        for command in &self.commands {
            if !roots.contains(&command.root) && holds(&command.root) {
                roots.push(command.root.clone());
            }
        }

        roots
    }

    fn gates(&self, _: &dyn Tree, root: &Root) -> Vec<Gate> {
        self.commands
            .iter()
            .filter(|command| command.root == *root)
            .map(|command| Gate {
                name: command.name.clone(),
                setup: command.setup.clone(),
                run: command.run.clone(),
            })
            .collect()
    }

    fn fixes(&self, _: &dyn Tree, root: &Root) -> Vec<Fix> {
        self.commands
            .iter()
            .filter(|command| command.root == *root)
            .filter_map(|command| {
                command.fix.as_ref().map(|run| Fix {
                    name: command.name.clone(),
                    run: run.clone(),
                })
            })
            .collect()
    }
}
