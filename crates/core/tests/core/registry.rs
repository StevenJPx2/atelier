//! Plugins register the roles they fill, and projects find them by name.

use crate::world::World;
use atelier_core::{Plugin, Registry, Setting, Settings};

struct Both(World);

struct AgentOnly(World);

impl Plugin for Both {
    fn name(&self) -> &'static str {
        "both"
    }

    fn register<'a>(&'a self, registry: &mut Registry<'a>) {
        registry.tracker(|settings| {
            settings.only("", &["site"])?;
            Ok(Box::new(World::default()))
        });
        registry.forge(&self.0);
    }
}

impl Plugin for AgentOnly {
    fn name(&self) -> &'static str {
        "agent"
    }

    fn register<'a>(&'a self, registry: &mut Registry<'a>) {
        registry.agent(&self.0);
    }
}

#[test]
fn a_plugin_is_found_only_in_the_roles_it_registered() {
    let both = Both(World::default());
    let agent = AgentOnly(World::default());
    let registry = Registry::new(&[&both, &agent]);

    assert!(registry.find_tracker("both").is_ok());
    assert!(registry.find_forge("both").is_ok());
    assert!(registry.find_agent(Some("agent")).is_ok());
    assert!(registry.find_agent(None).is_ok());

    let missing = [
        (
            registry.find_tracker("agent").err(),
            "this atelier has no tracker plugin \"agent\"; it has both",
        ),
        (
            registry.find_forge("jira").err(),
            "this atelier has no forge plugin \"jira\"; it has both",
        ),
        (
            registry.find_agent(Some("both")).err(),
            "this atelier has no agent plugin \"both\"; it has agent",
        ),
    ];

    for (error, message) in missing {
        assert_eq!(
            error.map(|error| error.to_string()).as_deref(),
            Some(message)
        );
    }
}

#[test]
fn with_no_agent_or_two_a_project_must_name_one() {
    let first = AgentOnly(World::default());
    let second = AgentOnly(World::default());

    let none = Registry::new(&[]);
    let two = Registry::new(&[&first, &second]);

    let errors = [none.find_agent(None).err(), two.find_agent(None).err()];

    assert_eq!(
        errors.map(|error| error.map(|error| error.to_string())),
        [
            Some("name the agent plugin to use; this atelier has none".to_owned()),
            Some("name the agent plugin to use; this atelier has agent, agent".to_owned()),
        ]
    );
}

#[test]
fn each_project_builds_its_own_tracker_from_its_settings() {
    let both = Both(World::default());
    let registry = Registry::new(&[&both]);
    let make = registry.find_tracker("both").unwrap();

    let fine = Settings::default().with("site", Setting::Text("example.org".into()));
    let misspelt = Settings::default().with("stie", Setting::Text("example.org".into()));

    assert!(make.make(&fine).is_ok());
    assert_eq!(
        make.make(&misspelt)
            .err()
            .map(|error| error.to_string())
            .as_deref(),
        Some("stie: unknown; it takes site")
    );
}
