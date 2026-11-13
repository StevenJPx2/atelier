//! The agent's kind of step, `session`.

use atelier_core::{
    Kind, MemoryTree, Outcomes, Plugin, Registry, Setting, Settings, SettingsError, Step,
};
use atelier_opencode::OpenCode;

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
