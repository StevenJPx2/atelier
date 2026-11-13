//! Reading a pipeline from its steps' settings, and each way the config
//! check refuses one.

use atelier_core::{
    Kind, Kinds, MemoryTree, Need, Outcomes, Pipeline, Registry, Setting, Settings, SettingsError,
    Step, Tree,
};

/// Kinds as the build's plugins offer them, with an `opencode` whose
/// `session` reads its prompt from the tree.
struct Offers {
    kinds: Vec<Kind>,
}

const PASSED: Outcomes = Outcomes::Own(&["passed"]);

const fn kind(name: &'static str, outcomes: Outcomes, resident: bool) -> Kind {
    Kind {
        name,
        outcomes,
        resident,
    }
}

impl Step for Offers {
    fn kinds(&self) -> Vec<Kind> {
        self.kinds.clone()
    }

    fn configure(
        &self,
        kind: &str,
        settings: &Settings,
        tree: &dyn Tree,
    ) -> Result<Settings, SettingsError> {
        match kind {
            "session" => {
                settings.only("", &["prompt", "brief"])?;
                let prompt = settings.text("", "prompt")?.unwrap_or_default();

                match tree.read(prompt) {
                    Some(text) => Ok(settings.clone().with("text", Setting::Text(text))),
                    None if settings.text("", "brief")? == Some("builder") => Ok(settings.clone()),
                    None => Err(SettingsError::new("", "prompt", "is not on origin/main")),
                }
            }
            "command" => {
                settings.only("", &["run"])?;
                settings
                    .text("", "run")?
                    .ok_or_else(|| SettingsError::new("", "run", "must be a command"))?;
                Ok(settings.clone())
            }
            _ => {
                settings.only("", &[])?;
                Ok(settings.clone())
            }
        }
    }
}

struct Build {
    git: Offers,
    github: Offers,
    opencode: Offers,
    atelier: Offers,
}

impl Build {
    fn new() -> Self {
        Self {
            git: Offers {
                kinds: vec![kind("worktree", PASSED, false)],
            },
            github: Offers {
                kinds: vec![
                    kind("checks", PASSED, false),
                    kind("ready", PASSED, false),
                    kind("merge", Outcomes::Own(&["merged"]), false),
                ],
            },
            opencode: Offers {
                kinds: vec![kind("session", Outcomes::Named, true)],
            },
            atelier: Offers {
                kinds: vec![kind("command", PASSED, false)],
            },
        }
    }

    fn kinds(&self) -> Kinds<'_> {
        Kinds::default()
            .with("git", "git", &self.git)
            .with("github", "github", &self.github)
            .with("forge", "github", &self.github)
            .with("opencode", "opencode", &self.opencode)
            .with("agent", "opencode", &self.opencode)
            .with("atelier", "atelier", &self.atelier)
    }
}

fn text(value: &str) -> Setting {
    Setting::Text(value.into())
}

fn list(values: &[&str]) -> Setting {
    Setting::List(values.iter().map(|value| text(value)).collect())
}

fn table(entries: &[(&str, Setting)]) -> Setting {
    Setting::Table(
        entries
            .iter()
            .fold(Settings::default(), |table, (key, value)| {
                table.with(key, value.clone())
            }),
    )
}

fn step(id: &str, uses: &str, rest: &[(&str, Setting)]) -> Settings {
    rest.iter().fold(
        Settings::default()
            .with("id", text(id))
            .with("uses", text(uses)),
        |step, (key, value)| step.with(key, value.clone()),
    )
}

fn tree() -> MemoryTree {
    MemoryTree::new([(".atelier/prompts/audit.md", "Audit the dependencies.")])
}

fn read(steps: &[Settings]) -> Result<Pipeline, String> {
    let build = Build::new();

    Pipeline::read("security-audit", steps, &build.kinds(), &tree())
        .map_err(|error| error.to_string())
}

fn security_audit() -> Vec<Settings> {
    vec![
        step("worktree", "git.worktree", &[]),
        step(
            "deps",
            "atelier.command",
            &[
                ("run", text("cargo deny check advisories")),
                ("after", list(&["worktree"])),
            ],
        ),
        step(
            "audit",
            "agent.session",
            &[
                ("prompt", text(".atelier/prompts/audit.md")),
                ("outcomes", list(&["found", "clean"])),
                ("after", list(&["worktree"])),
            ],
        ),
        step(
            "fix",
            "agent.session",
            &[
                ("prompt", text(".atelier/prompts/audit.md")),
                ("outcomes", list(&["opened"])),
                (
                    "after_any",
                    table(&[("audit", text("found")), ("deps", text("failed"))]),
                ),
            ],
        ),
    ]
}

fn need(step: &str, outcomes: &[&str]) -> Need {
    Need {
        step: step.into(),
        outcomes: outcomes.iter().map(|&outcome| outcome.to_owned()).collect(),
    }
}

#[test]
fn a_pipeline_reads_each_step_its_kind_its_needs_and_its_own_settings() {
    let pipeline = read(&security_audit()).unwrap();
    let ids: Vec<&str> = pipeline
        .steps()
        .iter()
        .map(|step| step.id.as_str())
        .collect();
    let audit = pipeline.step("audit").unwrap();
    let fix = pipeline.step("fix").unwrap();

    assert_eq!(pipeline.name(), "security-audit");
    assert_eq!(ids, ["worktree", "deps", "audit", "fix"]);
    assert_eq!(audit.resolved(), "opencode.session");
    assert_eq!(audit.uses, "agent.session");
    assert!(audit.resident);
    assert_eq!(audit.outcomes, ["found", "clean"]);
    assert_eq!(audit.needs.all, [need("worktree", &["passed"])]);
    assert_eq!(
        audit.settings.text("", "text").unwrap(),
        Some("Audit the dependencies.")
    );
    assert_eq!(fix.needs.all, []);
    assert_eq!(
        fix.needs.any,
        [need("audit", &["found"]), need("deps", &["failed"])]
    );
    assert_eq!(pipeline.step("deps").unwrap().outcomes, ["passed"]);
    assert_eq!(pipeline.step("deps").unwrap().rounds, 1);
    assert!(!pipeline.step("deps").unwrap().restart);
    assert_eq!(
        pipeline.step("deps").unwrap().settings,
        Settings::default().with("run", text("cargo deny check advisories"))
    );
}

#[test]
fn a_step_with_no_needs_written_waits_for_the_one_before_it_to_pass() {
    let pipeline = read(&[
        step("worktree", "git.worktree", &[]),
        step(
            "checks",
            "forge.checks",
            &[
                ("rounds", Setting::Number(5)),
                ("restart", Setting::Flag(true)),
            ],
        ),
        step("ready", "github.ready", &[("after", list(&[]))]),
        step("merge", "forge.merge", &[]),
    ])
    .unwrap();
    let checks = pipeline.step("checks").unwrap();

    assert_eq!(pipeline.step("worktree").unwrap().needs.all, []);
    assert_eq!(checks.needs.all, [need("worktree", &["passed"])]);
    assert_eq!(checks.rounds, 5);
    assert!(checks.restart);
    assert_eq!(checks.resolved(), "github.checks");
    assert_eq!(pipeline.step("ready").unwrap().needs.all, []);
    assert_eq!(pipeline.step("merge").unwrap().outcomes, ["merged"]);
    assert_eq!(
        pipeline.step("merge").unwrap().needs.all,
        [need("ready", &["passed"])]
    );
}

#[test]
fn a_pipeline_of_eight_steps_is_read_and_a_kind_s_own_outcomes_may_be_written() {
    let mut steps = vec![step(
        "s0",
        "git.worktree",
        &[("outcomes", list(&["passed"]))],
    )];
    steps.extend((1..8).map(|index| step(&format!("s{index}"), "git.worktree", &[])));

    let pipeline = read(&steps).unwrap();

    assert_eq!(pipeline.steps().len(), 8);
    assert_eq!(pipeline.step("s0").unwrap().outcomes, ["passed"]);
}

#[test]
fn a_table_names_one_outcome_or_several() {
    let pipeline = read(&[
        step("worktree", "git.worktree", &[]),
        step(
            "audit",
            "agent.session",
            &[
                ("brief", text("builder")),
                ("outcomes", list(&["found", "clean"])),
            ],
        ),
        step(
            "report",
            "atelier.command",
            &[
                ("run", text("true")),
                (
                    "after",
                    table(&[("audit", list(&["found", "clean", "failed"]))]),
                ),
            ],
        ),
    ])
    .unwrap();

    assert_eq!(
        pipeline.step("report").unwrap().needs.all,
        [need("audit", &["found", "clean", "failed"])]
    );
}

/// Each refusal reads as a person sees it in `atelier status`.
fn refuses(cases: Vec<(Vec<Settings>, &str)>) {
    for (steps, message) in cases {
        assert_eq!(read(&steps).err().as_deref(), Some(message));
    }
}

fn worktree() -> Settings {
    step("worktree", "git.worktree", &[])
}

fn audit(rest: &[(&str, Setting)]) -> Settings {
    let mut settings = vec![("brief", text("builder"))];
    settings.extend(rest.iter().cloned());
    step("audit", "agent.session", &settings)
}

#[test]
fn a_pipeline_needs_a_name_and_one_to_eight_steps() {
    let nine: Vec<Settings> = (0..9)
        .map(|index| step(&format!("s{index}"), "git.worktree", &[]))
        .collect();
    let build = Build::new();
    let named = Pipeline::read("security audit", &[worktree()], &build.kinds(), &tree());

    refuses(vec![
        (vec![], "security-audit: it has no steps"),
        (nine, "security-audit: 9 steps; a pipeline has at most 8"),
    ]);
    assert_eq!(
        named.unwrap_err().to_string(),
        "security audit: a pipeline's name takes letters, digits, - and _"
    );
}

#[test]
fn a_step_needs_its_own_id_and_a_kind_this_atelier_offers() {
    refuses(vec![
        (
            vec![Settings::default().with("uses", text("git.worktree"))],
            "security-audit: step 1 has no id",
        ),
        (
            vec![step("work tree", "git.worktree", &[])],
            "security-audit: work tree: an id takes letters, digits, - and _",
        ),
        (
            vec![worktree(), worktree()],
            "security-audit: two steps are called worktree",
        ),
        (
            vec![Settings::default().with("id", text("worktree"))],
            "security-audit: worktree: uses is missing; it names a step kind, such as git.worktree",
        ),
        (
            vec![step("worktree", "worktree", &[])],
            "security-audit: worktree: uses is \"worktree\"; it takes <namespace>.<kind>, such as forge.checks",
        ),
        (
            vec![step("worktree", "git.", &[])],
            "security-audit: worktree: uses is \"git.\"; it takes <namespace>.<kind>, such as forge.checks",
        ),
        (
            vec![step("worktree", ".worktree", &[])],
            "security-audit: worktree: uses is \".worktree\"; it takes <namespace>.<kind>, such as forge.checks",
        ),
        (
            vec![worktree(), step("reviews", "forge.reviews", &[])],
            "security-audit: no step kind \"forge.reviews\"; github has checks, ready, merge",
        ),
        (
            vec![step("lint", "rust.clippy", &[])],
            "security-audit: no step kind \"rust.clippy\"; there is no plugin or role rust, only agent, atelier, forge, git, github, opencode",
        ),
    ]);
}

#[test]
fn outcomes_rounds_and_restart_are_checked() {
    refuses(vec![
        (
            vec![step(
                "worktree",
                "git.worktree",
                &[("outcomes", list(&["made"]))],
            )],
            "security-audit: worktree: outcomes are git.worktree's own: passed",
        ),
        (
            vec![worktree(), audit(&[("outcomes", list(&[]))])],
            "security-audit: audit: outcomes is empty; name at least one",
        ),
        (
            vec![
                worktree(),
                audit(&[("outcomes", list(&["clean", "failed"]))]),
            ],
            "security-audit: audit: every step can settle failed; outcomes names the others",
        ),
        (
            vec![worktree(), audit(&[("outcomes", list(&["all clear"]))])],
            "security-audit: audit: an outcome takes letters, digits, - and _",
        ),
        (
            vec![
                worktree(),
                audit(&[("outcomes", list(&["clean", "clean"]))]),
            ],
            "security-audit: audit: outcomes names clean twice",
        ),
        (
            vec![worktree(), audit(&[("outcomes", text("found"))])],
            "security-audit: audit.outcomes: must be a list of text",
        ),
        (
            vec![step(
                "worktree",
                "git.worktree",
                &[("rounds", Setting::Number(0))],
            )],
            "security-audit: worktree: rounds is 0; it must be from 1 to 255",
        ),
        (
            vec![step(
                "worktree",
                "git.worktree",
                &[("rounds", Setting::Number(256))],
            )],
            "security-audit: worktree: rounds is 256; it must be from 1 to 255",
        ),
        (
            vec![step(
                "worktree",
                "git.worktree",
                &[("rounds", text("five"))],
            )],
            "security-audit: worktree.rounds: must be a number",
        ),
        (
            vec![step(
                "worktree",
                "git.worktree",
                &[("restart", text("yes"))],
            )],
            "security-audit: worktree.restart: must be true or false",
        ),
    ]);
}

#[test]
fn a_step_waits_only_on_outcomes_of_steps_before_it() {
    let ready = |after: Setting| step("ready", "forge.ready", &[("after", after)]);
    let wrong =
        "security-audit: ready.after: takes a list of steps, or a table of each step's outcomes";

    refuses(vec![
        (
            vec![step(
                "worktree",
                "git.worktree",
                &[("after", list(&["worktree"]))],
            )],
            "security-audit: worktree: after names itself; a step waits only on steps before it",
        ),
        (
            vec![
                step("checks", "forge.checks", &[("after", list(&["merge"]))]),
                step("merge", "forge.merge", &[]),
            ],
            "security-audit: checks: after names merge, which is no step before it; a step waits only on steps before it",
        ),
        (
            vec![
                worktree(),
                audit(&[("after_any", table(&[("audti", text("found"))]))]),
            ],
            "security-audit: audit: after_any names audti, which is no step before it; a step waits only on steps before it",
        ),
        (
            vec![worktree(), ready(table(&[("worktree", text("made"))]))],
            "security-audit: ready: after names worktree's made; worktree settles passed, or failed",
        ),
        (
            vec![worktree(), ready(table(&[("worktree", list(&[]))]))],
            "security-audit: ready.after: names no outcome of worktree",
        ),
        (vec![worktree(), ready(text("worktree"))], wrong),
        (
            vec![worktree(), ready(Setting::List(vec![Setting::Number(1)]))],
            wrong,
        ),
    ]);
}

#[test]
fn a_kind_refuses_settings_it_cannot_use() {
    refuses(vec![
        (
            vec![worktree(), step("deps", "atelier.command", &[])],
            "security-audit: deps.run: must be a command",
        ),
        (
            vec![
                worktree(),
                step(
                    "deps",
                    "atelier.command",
                    &[("run", text("true")), ("timeout", Setting::Number(5))],
                ),
            ],
            "security-audit: deps.timeout: unknown; it takes run",
        ),
        (
            vec![
                worktree(),
                step("audit", "agent.session", &[("prompt", text("missing.md"))]),
            ],
            "security-audit: audit.prompt: is not on origin/main",
        ),
    ]);
}

/// The registry puts each plugin's kinds under its name, and the project's
/// agent, forge and only tracker under their roles.
#[test]
fn a_role_names_the_plugin_that_fills_it_in_this_project() {
    struct Named(&'static str, Offers);

    impl atelier_core::Plugin for Named {
        fn name(&self) -> &'static str {
            self.0
        }

        fn register<'a>(&'a self, registry: &mut Registry<'a>) {
            registry.step(&self.1);
        }
    }

    let build = Build::new();
    let github = Named("github", build.github);
    let opencode = Named("opencode", build.opencode);
    let registry = Registry::new(&[&github, &opencode]);
    let checks = |kinds: &Kinds<'_>| {
        Pipeline::read(
            "build",
            &[
                step("checks", "forge.checks", &[]),
                step("issue", "tracker.checks", &[]),
            ],
            kinds,
            &tree(),
        )
        .map(|pipeline| {
            pipeline
                .steps()
                .iter()
                .map(atelier_core::StepSpec::resolved)
                .collect::<Vec<_>>()
        })
        .map_err(|error| error.to_string())
    };

    assert_eq!(
        checks(&registry.kinds(None, "github", &["github"])),
        Ok(vec!["github.checks".to_owned(), "github.checks".to_owned()])
    );
    assert_eq!(
        checks(&registry.kinds(Some("opencode"), "gitlab", &["github", "jira"])),
        Err("build: no step kind \"forge.checks\"; there is no plugin or role forge, only agent, github, opencode".to_owned())
    );
}
