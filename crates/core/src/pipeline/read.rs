use super::{FAILED, Kinds, MAX_STEPS, Need, Needs, PASSED, Pipeline, PipelineError, StepSpec};
use crate::{Outcomes, Setting, Settings, Tree};

/// The keys every step has; the rest are its kind's own.
const SHARED: [&str; 7] = [
    "id",
    "uses",
    "after",
    "after_any",
    "outcomes",
    "rounds",
    "restart",
];

impl Pipeline {
    /// Reads and checks the pipeline `name` from its steps' settings, as
    /// `[[pipeline.<name>.step]]` writes them, each step's own settings
    /// configured by the plugin whose kind it uses.
    ///
    /// ```
    /// use atelier_core::{Kind, Kinds, MemoryTree, Outcomes, Pipeline, Setting, Settings,
    ///                    SettingsError, Step, Tree};
    ///
    /// struct Git;
    ///
    /// impl Step for Git {
    ///     fn kinds(&self) -> Vec<Kind> {
    ///         vec![Kind { name: "worktree", outcomes: Outcomes::Own(&["passed"]), resident: false }]
    ///     }
    ///
    ///     fn configure(&self, _: &str, settings: &Settings, _: &dyn Tree)
    ///         -> Result<Settings, SettingsError> {
    ///         Ok(settings.clone())
    ///     }
    /// }
    ///
    /// let kinds = Kinds::default().with("git", "git", &Git);
    /// let worktree = Settings::default()
    ///     .with("id", Setting::Text("worktree".into()))
    ///     .with("uses", Setting::Text("git.wroktree".into()));
    ///
    /// let error = Pipeline::read("audit", &[worktree], &kinds, &MemoryTree::default()).unwrap_err();
    ///
    /// assert_eq!(error.to_string(), r#"audit: no step kind "git.wroktree"; git has worktree"#);
    /// ```
    ///
    /// # Errors
    ///
    /// No steps or more than eight, a step without an id or kind, an
    /// unknown kind, a need on a step that is not before it or on an
    /// outcome that step cannot settle with, or a setting its kind cannot
    /// use.
    pub fn read(
        name: &str,
        steps: &[Settings],
        kinds: &Kinds<'_>,
        tree: &dyn Tree,
    ) -> Result<Self, PipelineError> {
        let refuse = |problem: String| PipelineError {
            pipeline: name.to_owned(),
            problem,
        };

        if !named(name) {
            return Err(refuse(
                "a pipeline's name takes letters, digits, - and _".into(),
            ));
        }

        if steps.is_empty() {
            return Err(refuse("it has no steps".into()));
        }

        if steps.len() > MAX_STEPS {
            return Err(refuse(format!(
                "{} steps; a pipeline has at most {MAX_STEPS}",
                steps.len()
            )));
        }

        let mut read: Vec<StepSpec> = Vec::new();

        for (index, settings) in steps.iter().enumerate() {
            let step = one(index, settings, &read, kinds, tree).map_err(refuse)?;
            read.push(step);
        }

        let plan = super::lower::lower(&read).map_err(refuse)?;

        Ok(Self {
            name: name.to_owned(),
            steps: read,
            plan,
        })
    }
}

/// Reads the step at `index`, given the steps before it.
fn one(
    index: usize,
    settings: &Settings,
    before: &[StepSpec],
    kinds: &Kinds<'_>,
    tree: &dyn Tree,
) -> Result<StepSpec, String> {
    let text = |key: &str| settings.text("", key).map_err(|error| error.to_string());
    let number = index.saturating_add(1);

    let id = text("id")?.ok_or_else(|| format!("step {number} has no id"))?;

    if !named(id) {
        return Err(format!("{id}: an id takes letters, digits, - and _"));
    }

    if before.iter().any(|step| step.id == id) {
        return Err(format!("two steps are called {id}"));
    }

    let mistyped = |error: crate::SettingsError| format!("{id}.{error}");
    let uses = settings
        .text("", "uses")
        .map_err(mistyped)?
        .ok_or_else(|| {
            format!("{id}: uses is missing; it names a step kind, such as git.worktree")
        })?;

    let (namespace, kind_name) = uses
        .split_once('.')
        .filter(|(namespace, kind)| !namespace.is_empty() && !kind.is_empty())
        .ok_or_else(|| {
            format!("{id}: uses is \"{uses}\"; it takes <namespace>.<kind>, such as forge.checks")
        })?;

    let (plugin, step) = kinds.find(namespace).ok_or_else(|| {
        format!(
            "no step kind \"{uses}\"; there is no plugin or role {namespace}, only {}",
            kinds.names().join(", ")
        )
    })?;

    let kind = Kinds::kind(step, kind_name).ok_or_else(|| {
        let offered: Vec<&str> = step.kinds().iter().map(|kind| kind.name).collect();
        format!(
            "no step kind \"{uses}\"; {plugin} has {}",
            offered.join(", ")
        )
    })?;

    let outcomes = outcomes(id, uses, kind.outcomes, settings)?;

    let rounds = match settings.number("", "rounds").map_err(mistyped)? {
        None => 1,
        Some(rounds) => u8::try_from(rounds)
            .ok()
            .filter(|rounds| *rounds > 0)
            .ok_or_else(|| format!("{id}: rounds is {rounds}; it must be from 1 to 255"))?,
    };

    let restart = settings
        .flag("", "restart")
        .map_err(mistyped)?
        .unwrap_or(false);

    let needs = needs(id, settings, before)?;

    let own = step
        .configure(kind_name, &settings.without(&SHARED), tree)
        .map_err(mistyped)?;

    Ok(StepSpec {
        id: id.to_owned(),
        uses: uses.to_owned(),
        plugin,
        kind: kind_name.to_owned(),
        needs,
        outcomes,
        rounds,
        restart,
        resident: kind.resident,
        settings: own,
    })
}

/// The step's outcomes: its kind's own, or those it names.
fn outcomes(
    id: &str,
    uses: &str,
    kind: Outcomes,
    settings: &Settings,
) -> Result<Vec<String>, String> {
    let written = match settings.get("outcomes") {
        None => None,
        Some(_) => Some(
            settings
                .texts("", "outcomes")
                .map_err(|error| format!("{id}.{error}"))?,
        ),
    };

    let outcomes: Vec<String> = match (kind, written) {
        (Outcomes::Own(own), None) => own.iter().map(|&outcome| outcome.to_owned()).collect(),
        (Outcomes::Own(own), Some(written)) if written == own => {
            own.iter().map(|&outcome| outcome.to_owned()).collect()
        }
        (Outcomes::Own(own), Some(_)) => {
            return Err(format!(
                "{id}: outcomes are {uses}'s own: {}",
                own.join(", ")
            ));
        }
        (Outcomes::Named, None) => vec![PASSED.to_owned()],
        (Outcomes::Named, Some(written)) => written.into_iter().map(str::to_owned).collect(),
    };

    if outcomes.is_empty() {
        return Err(format!("{id}: outcomes is empty; name at least one"));
    }

    for (index, outcome) in outcomes.iter().enumerate() {
        if outcome == FAILED {
            return Err(format!(
                "{id}: every step can settle failed; outcomes names the others"
            ));
        }

        if !named(outcome) {
            return Err(format!("{id}: an outcome takes letters, digits, - and _"));
        }

        if outcomes
            .iter()
            .take(index)
            .any(|earlier| earlier == outcome)
        {
            return Err(format!("{id}: outcomes names {outcome} twice"));
        }
    }

    Ok(outcomes)
}

/// The step's `after` and `after_any`. With neither, it waits for the
/// step before it to pass.
fn needs(id: &str, settings: &Settings, before: &[StepSpec]) -> Result<Needs, String> {
    let all = settings.get("after");
    let any = settings.get("after_any");

    let needs = match (all, any, before.last()) {
        (None, None, Some(last)) => Needs {
            all: vec![Need {
                step: last.id.clone(),
                outcomes: vec![PASSED.to_owned()],
            }],
            any: Vec::new(),
        },
        _ => Needs {
            all: all.map_or(Ok(Vec::new()), |all| written(id, "after", all))?,
            any: any.map_or(Ok(Vec::new()), |any| written(id, "after_any", any))?,
        },
    };

    for (key, list) in [("after", &needs.all), ("after_any", &needs.any)] {
        for need in list {
            check(id, key, need, before)?;
        }
    }

    Ok(needs)
}

/// One of `after` or `after_any` as written: a list of steps that must
/// pass, or a table of each step's outcomes, one or a list.
fn written(id: &str, key: &str, setting: &Setting) -> Result<Vec<Need>, String> {
    let wrong = || format!("{id}.{key}: takes a list of steps, or a table of each step's outcomes");
    let text = |setting: &Setting| match setting {
        Setting::Text(text) => Ok(text.clone()),
        _ => Err(wrong()),
    };

    match setting {
        Setting::List(steps) => steps
            .iter()
            .map(|step| {
                Ok(Need {
                    step: text(step)?,
                    outcomes: vec![PASSED.to_owned()],
                })
            })
            .collect(),
        Setting::Table(table) => table
            .iter()
            .map(|(step, outcomes)| {
                let outcomes = match outcomes {
                    Setting::List(list) => list.iter().map(text).collect::<Result<_, _>>()?,
                    one => vec![text(one)?],
                };

                if outcomes.is_empty() {
                    return Err(format!("{id}.{key}: names no outcome of {step}"));
                }

                Ok(Need {
                    step: step.to_owned(),
                    outcomes,
                })
            })
            .collect(),
        _ => Err(wrong()),
    }
}

/// A need names a step before this one, and outcomes that step has.
fn check(id: &str, key: &str, need: &Need, before: &[StepSpec]) -> Result<(), String> {
    let name = &need.step;

    let Some(step) = before.iter().find(|step| &step.id == name) else {
        let problem = if name == id {
            "itself; a step waits only on steps before it".to_owned()
        } else {
            format!("{name}, which is no step before it; a step waits only on steps before it")
        };

        return Err(format!("{id}: {key} names {problem}"));
    };

    if let Some(outcome) = need
        .outcomes
        .iter()
        .find(|outcome| *outcome != FAILED && !step.outcomes.contains(outcome))
    {
        return Err(format!(
            "{id}: {key} names {name}'s {outcome}; {name} settles {}, or {FAILED}",
            step.outcomes.join(", ")
        ));
    }

    Ok(())
}

/// Letters, digits, `-` and `_`, and at least one.
fn named(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}
