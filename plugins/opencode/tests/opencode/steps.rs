//! The agent's kind of step, `session`.

use std::path::Path;

use atelier_core::{
    Brief, Kind, MemoryTree, Needs, Observed, Outcome, Outcomes, Plugin, PullRequest, Registry,
    Repo, Rollup, RunContext, Setting, Settings, SettingsError, Started, Step, StepSpec,
};
use atelier_opencode::OpenCode;

use crate::{CREATED, Reply, StandIn, body, brief};

fn configure(settings: &[(&str, &str)]) -> Result<Settings, SettingsError> {
    let tree = MemoryTree::new([(".atelier/prompts/audit.md", "Audit the dependencies.")]);
    let settings = settings
        .iter()
        .fold(Settings::default(), |all, (key, value)| {
            all.with(key, Setting::Text((*value).into()))
        });

    OpenCode::new().configure("session", &settings, &tree)
}

#[test]
fn a_session_is_resident_and_settles_with_the_outcomes_its_step_names() {
    assert_eq!(
        OpenCode::new().kinds(),
        [Kind {
            name: "session",
            outcomes: Outcomes::Named,
            resident: true,
        }]
    );
}

#[test]
fn a_session_takes_a_prompt_on_the_base_branch_or_the_builder_s_brief() {
    let prompt = configure(&[("prompt", ".atelier/prompts/audit.md")]).unwrap();
    let builder = configure(&[("brief", "builder")]).unwrap();
    let refused = |settings: &[(&str, &str)]| configure(settings).unwrap_err().to_string();

    assert_eq!(
        prompt.text("", "text").unwrap(),
        Some("Audit the dependencies.")
    );
    assert_eq!(
        builder,
        Settings::default().with("brief", Setting::Text("builder".into()))
    );
    assert_eq!(
        refused(&[("prompt", "missing.md")]),
        "prompt: missing.md is not on the base branch"
    );
    assert_eq!(
        refused(&[("brief", "critic")]),
        "brief: must be \"builder\""
    );
    assert_eq!(
        refused(&[
            ("brief", "builder"),
            ("prompt", ".atelier/prompts/audit.md")
        ]),
        "brief: goes without prompt; a session takes one of them"
    );
    assert_eq!(
        refused(&[]),
        "prompt: is missing; a session takes a prompt file, or brief = \"builder\""
    );
    assert_eq!(
        refused(&[("brief", "builder"), ("model", "opus")]),
        "model: unknown; it takes prompt, brief"
    );
}

#[test]
fn opencode_registers_its_session_as_the_agent() {
    let opencode = OpenCode::new();
    let plugins: [&dyn Plugin; 1] = [&opencode];
    let registry = Registry::new(&plugins);
    let step = Settings::default()
        .with("id", Setting::Text("builder".into()))
        .with("uses", Setting::Text("agent.session".into()))
        .with("brief", Setting::Text("builder".into()));

    let pipeline = atelier_core::Pipeline::read(
        "build",
        &[step],
        &registry.kinds(None, "github", &["github"]),
        &MemoryTree::default(),
    )
    .unwrap();

    assert_eq!(pipeline.steps()[0].resolved(), "opencode.session");
}
/// A session step with `settings`, as the config checked it.
fn session(settings: Settings) -> StepSpec {
    StepSpec {
        id: "audit".into(),
        uses: "agent.session".into(),
        plugin: "opencode",
        kind: "session".into(),
        needs: Needs::default(),
        outcomes: vec!["found".into(), "clean".into()],
        rounds: 1,
        restart: false,
        resident: true,
        settings,
    }
}

/// Calls `with` on a run in `worktree`, seeing `pull_request`.
fn with_run<T>(
    worktree: Option<&Path>,
    pull_request: Option<PullRequest>,
    with: impl FnOnce(&RunContext<'_>) -> T,
) -> T {
    let brief: Brief = brief();
    let issue = atelier_core::Issue {
        key: atelier_core::IssueKey {
            tracker: "github".into(),
            id: "12".into(),
        },
        title: "Fix the dead link".into(),
        open: true,
        marks: atelier_core::Marks::default(),
    };
    let repo = Repo("StevenJPx2/chauffeur".into());
    let run = RunContext {
        repo: &repo,
        local: Path::new("/clones/chauffeur.git"),
        base: "v2",
        issue: &issue,
        name: "issue-12",
        branch: "issue-12-fix-the-dead-link",
        worktree,
        folder: Path::new("/steps/audit"),
        brief: &brief,
        session: None,
        pull_request,
        failed: 0,
        settled: false,
    };

    with(&run)
}

fn text(value: &str) -> Setting {
    Setting::Text(value.into())
}

#[test]
fn a_session_starts_with_its_prompt_or_the_builder_s_brief() {
    let worktree = Path::new("/worktrees/issue-12");
    let replies = [
        Reply::Ok("session-list-empty.json"),
        Reply::Ok("session-created.json"),
        Reply::Ok("interrupted-idle.json"),
    ];
    let prompted = StandIn::new(&replies);
    let builder = StandIn::new(&replies);
    let audit = session(
        Settings::default()
            .with("prompt", text(".atelier/prompts/audit.md"))
            .with("text", text("Audit the dependencies.")),
    );
    let brief_step = session(Settings::default().with("brief", text("builder")));

    let started = with_run(Some(worktree), None, |run| {
        prompted.opencode().start(run, &audit)
    });
    let built = with_run(Some(worktree), None, |run| {
        builder.opencode().start(run, &brief_step)
    });
    let homeless = with_run(None, None, |run| OpenCode::new().start(run, &audit));

    let Ok(Started::Session(made)) = started else {
        panic!("no session: {started:?}");
    };
    assert_eq!(made.id, CREATED);
    assert_eq!(made.title, "#12 Fix the dead link [audit]");
    assert_eq!(body(&prompted.calls()[1])["title"], made.title);
    assert_eq!(body(&builder.calls()[1])["title"], "#12 Fix the dead link");
    assert_eq!(
        body(&prompted.calls()[2])["text"],
        "Audit the dependencies."
    );
    assert_eq!(body(&builder.calls()[2])["text"], brief().text());
    assert!(matches!(built, Ok(Started::Session(_))));
    assert!(homeless.unwrap_err().is_permanent());
}

#[test]
fn the_builder_settles_opened_with_its_pull_request_and_a_prompted_session_waits() {
    let open = PullRequest::Open {
        number: 8,
        draft: true,
        failed_rounds: 0,
        checks: Rollup::Pending,
    };
    let builder = session(Settings::default().with("brief", text("builder")));
    let audit = session(Settings::default().with("text", text("Audit.")));

    let opened = with_run(None, Some(open), |run| {
        OpenCode::new().observe(run, &builder)
    });
    let waiting = with_run(None, None, |run| OpenCode::new().observe(run, &builder));
    let prompted = with_run(None, Some(open), |run| OpenCode::new().observe(run, &audit));

    assert_eq!(
        opened.unwrap(),
        Observed::Settled(Outcome {
            name: "opened".into(),
            why: "opened #8".into(),
        })
    );
    assert_eq!(waiting.unwrap(), Observed::Running);
    assert_eq!(prompted.unwrap(), Observed::Running);
}
