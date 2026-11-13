//! The forge's kinds of step.

use atelier_core::{Kind, MemoryTree, Outcomes, Plugin, Registry, Setting, Settings, Step};
use atelier_github::GitHub;

#[test]
fn the_forge_offers_checks_ready_and_merge_which_take_no_settings() {
    let github = GitHub::new();
    let tree = MemoryTree::default();
    let extra = Settings::default().with("rounds_label", Setting::Text("x".into()));
    let names: Vec<(&str, Outcomes, bool)> = github
        .kinds()
        .into_iter()
        .map(
            |Kind {
                 name,
                 outcomes,
                 resident,
             }| (name, outcomes, resident),
        )
        .collect();

    assert_eq!(
        names,
        [
            ("checks", Outcomes::Own(&["passed"]), false),
            ("ready", Outcomes::Own(&["passed"]), false),
            ("merge", Outcomes::Own(&["merged"]), false),
        ]
    );

    for kind in ["checks", "ready", "merge"] {
        assert_eq!(
            github.configure(kind, &Settings::default(), &tree).unwrap(),
            Settings::default()
        );
        assert_eq!(
            github
                .configure(kind, &extra, &tree)
                .unwrap_err()
                .to_string(),
            "rounds_label: unknown; it takes no settings"
        );
    }
}

#[test]
fn github_registers_its_kinds_as_the_forge() {
    let github = GitHub::new();
    let plugins: [&dyn Plugin; 1] = [&github];
    let registry = Registry::new(&plugins);
    let step = Settings::default()
        .with("id", Setting::Text("checks".into()))
        .with("uses", Setting::Text("forge.checks".into()));

    let pipeline = atelier_core::Pipeline::read(
        "build",
        &[step],
        &registry.kinds(None, "github", &["github"]),
        &MemoryTree::default(),
    )
    .unwrap();

    assert_eq!(pipeline.steps()[0].resolved(), "github.checks");
}
