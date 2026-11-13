//! The `opencode` plugin through a stand-in `opencode` that replays what
//! the real one printed (see `fixtures/README.md`) and records each call,
//! and once, on demand, through the real one.

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "test support: a fixture that breaks fails the test that uses it"
)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use atelier_core::{
    Agent, Brief, Failure, Issue, IssueKey, Limits, Marks, Plugin, Registry, Session,
};
use atelier_opencode::{OpenCode, rules};

mod live;
mod steps;
use serde_json::Value;

static NEXT: AtomicU32 = AtomicU32::new(0);

const SCRIPT: &str = r#"#!/bin/sh
dir=$(dirname "$0")
n=$(( $(cat "$dir/count") + 1 ))
echo "$n" > "$dir/count"
for arg in "$@"; do printf '%s\n' "$arg"; done > "$dir/call-$n"
[ -f "$dir/out-$n" ] && cat "$dir/out-$n"
[ -f "$dir/err-$n" ] && cat "$dir/err-$n" >&2
exit "$(cat "$dir/exit-$n" 2>/dev/null || echo 0)"
"#;

/// One call's reply: what it prints, and whether it succeeds.
enum Reply {
    Ok(&'static str),
    Failed {
        stdout: &'static str,
        stderr: &'static str,
    },
    Unreachable,
}

struct StandIn {
    dir: PathBuf,
}

impl StandIn {
    fn new(replies: &[Reply]) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("atelier-oc-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("count"), "0").unwrap();
        std::fs::write(dir.join("opencode"), SCRIPT).unwrap();
        std::fs::set_permissions(dir.join("opencode"), std::fs::Permissions::from_mode(0o755))
            .unwrap();

        for (index, reply) in replies.iter().enumerate() {
            let n = index + 1;
            let (stdout, stderr, exit) = match reply {
                Reply::Ok(name) => (Some(*name), None, "0"),
                Reply::Failed { stdout, stderr } => (Some(*stdout), Some(*stderr), "1"),
                Reply::Unreachable => (None, None, "1"),
            };

            if let Some(name) = stdout {
                std::fs::write(dir.join(format!("out-{n}")), fixture(name)).unwrap();
            }

            if let Some(name) = stderr {
                std::fs::write(dir.join(format!("err-{n}")), fixture(name)).unwrap();
            }

            std::fs::write(dir.join(format!("exit-{n}")), exit).unwrap();
        }

        Self { dir }
    }

    fn opencode(&self) -> OpenCode {
        OpenCode::with_program(self.dir.join("opencode"))
    }

    fn calls(&self) -> Vec<Vec<String>> {
        let count: usize = std::fs::read_to_string(self.dir.join("count"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();

        (1..=count)
            .map(|n| {
                let text = std::fs::read_to_string(self.dir.join(format!("call-{n}"))).unwrap();
                text.lines().map(str::to_owned).collect()
            })
            .collect()
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/opencode/fixtures")
        .join(name);

    std::fs::read_to_string(path).unwrap()
}

fn brief() -> Brief {
    let issue = Issue {
        key: IssueKey {
            tracker: "github".into(),
            id: "12".into(),
        },
        title: "Fix the dead link".into(),
        open: true,
        marks: Marks::default(),
    };

    Brief::new(
        "#12",
        &issue,
        "issue-12-fix-the-dead-link",
        "v2",
        &Limits::new(5, []).unwrap(),
    )
    .with("Open a draft pull request.".into())
    .deny(["gh pr merge *".into()])
}

/// The JSON body a call sent with `--data`.
fn body(call: &[String]) -> Value {
    serde_json::from_str(
        &call[3..]
            .iter()
            .skip_while(|arg| *arg != "--data")
            .nth(1)
            .unwrap()
            .clone(),
    )
    .unwrap()
}

const CREATED: &str = "ses_e34e74974ffezeihakyxaDRnbK";

#[test]
fn a_builder_starts_as_a_session_in_its_worktree_with_its_rules_then_its_brief() {
    let oc = StandIn::new(&[
        Reply::Ok("session-list-empty.json"),
        Reply::Ok("session-created.json"),
        Reply::Ok("interrupted-idle.json"),
    ]);
    let brief = brief();
    let worktree = Path::new("/worktrees/StevenJPx2/chauffeur/issue-12");

    let session = oc.opencode().start(worktree, &brief, None).unwrap();

    let calls = oc.calls();
    let created = body(&calls[1]);
    assert_eq!(
        calls[0],
        [
            "api",
            "get",
            "/api/session?directory=/worktrees/StevenJPx2/chauffeur/issue-12"
        ]
    );
    assert_eq!(calls[1][..3], ["api", "post", "/api/session"]);
    assert_eq!(created["title"], "#12 Fix the dead link");
    assert_eq!(created["location"]["directory"], worktree.to_str().unwrap());
    assert_eq!(
        created["permissions"],
        serde_json::to_value(rules(&brief)).unwrap()
    );

    let permissions = created["permissions"].as_array().unwrap();
    for (action, resource, effect) in [
        ("shell", "gh pr merge *", "deny"),
        (
            "shell",
            "git push origin issue-12-fix-the-dead-link",
            "allow",
        ),
        ("question", "*", "deny"),
    ] {
        let rule = serde_json::json!({ "action": action, "resource": resource, "effect": effect });

        assert!(permissions.contains(&rule), "{rule}");
    }
    assert_eq!(
        calls[2][..3],
        ["api", "post", &format!("/api/session/{CREATED}/prompt")]
    );
    assert_eq!(body(&calls[2])["text"], brief.text());
    assert_eq!(
        session,
        Session {
            id: CREATED.into(),
            title: brief.title
        }
    );
}

/// The session in `session-list-one.json`.
const LISTED: &str = "ses_e2f334afbffeadMMO0ejknRlGJ";

#[test]
fn a_builder_already_briefed_in_its_worktree_is_continued_not_started_again() {
    let oc = StandIn::new(&[
        Reply::Ok("session-list-one.json"),
        Reply::Ok("messages-prompted.json"),
    ]);
    let brief = brief();

    let session = oc
        .opencode()
        .start(Path::new("/work trees/issue-12"), &brief, None)
        .unwrap();

    let calls = oc.calls();
    assert_eq!(
        calls[0],
        [
            "api",
            "get",
            "/api/session?directory=/work%20trees/issue-12"
        ]
    );
    assert_eq!(
        calls[1],
        ["api", "get", &format!("/api/session/{LISTED}/message")]
    );
    assert_eq!(calls.len(), 2);
    assert_eq!(
        session,
        Session {
            id: LISTED.into(),
            title: brief.title
        }
    );
}

#[test]
fn a_builder_made_but_never_briefed_gets_its_brief() {
    let oc = StandIn::new(&[
        Reply::Ok("session-list-one.json"),
        Reply::Ok("messages-empty.json"),
        Reply::Ok("prompt-accepted.json"),
    ]);
    let brief = brief();

    let session = oc.opencode().start(Path::new("/w"), &brief, None).unwrap();

    let calls = oc.calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls[2][..3],
        ["api", "post", &format!("/api/session/{LISTED}/prompt")]
    );
    assert_eq!(body(&calls[2])["text"], brief.text());
    assert_eq!(session.id, LISTED);
}

#[test]
fn the_builder_a_start_replaces_is_never_continued() {
    let oc = StandIn::new(&[
        Reply::Ok("session-list-one.json"),
        Reply::Ok("session-created.json"),
        Reply::Ok("prompt-accepted.json"),
    ]);
    let brief = brief();
    let stopped = Session {
        id: LISTED.into(),
        title: brief.title.clone(),
    };

    let session = oc
        .opencode()
        .start(Path::new("/w"), &brief, Some(&stopped))
        .unwrap();

    assert_eq!(oc.calls()[1][..3], ["api", "post", "/api/session"]);
    assert_eq!(session.id, CREATED);
}

#[test]
fn a_session_titled_for_another_issue_is_not_continued() {
    let oc = StandIn::new(&[
        Reply::Ok("session-list-one.json"),
        Reply::Ok("session-created.json"),
        Reply::Ok("prompt-accepted.json"),
    ]);
    let mut brief = brief();
    brief.title = "#13 Another issue".into();

    let session = oc.opencode().start(Path::new("/w"), &brief, None).unwrap();

    assert_eq!(oc.calls()[1][..3], ["api", "post", "/api/session"]);
    assert_eq!(session.id, CREATED);
}

#[test]
fn a_session_whose_prompt_fails_is_deleted() {
    let oc = StandIn::new(&[
        Reply::Ok("session-list-empty.json"),
        Reply::Ok("session-created.json"),
        Reply::Unreachable,
        Reply::Ok("interrupted-idle.json"),
    ]);

    let error = oc
        .opencode()
        .start(Path::new("/w"), &brief(), None)
        .unwrap_err();

    let calls = oc.calls();
    assert_eq!(error.failure, Failure::Transient);
    assert_eq!(
        calls[3],
        ["api", "delete", &format!("/api/session/{CREATED}")]
    );
}

#[test]
fn interrupting_succeeds_on_an_idle_session_and_on_one_that_is_gone() {
    let oc = StandIn::new(&[
        Reply::Ok("interrupted-idle.json"),
        Reply::Failed {
            stdout: "session-not-found.json",
            stderr: "session-not-found.stderr",
        },
    ]);
    let opencode = oc.opencode();
    let idle = Session {
        id: CREATED.into(),
        title: "#12".into(),
    };
    let gone = Session {
        id: "ses_doesnotexist".into(),
        title: "#13".into(),
    };

    opencode.interrupt(&idle).unwrap();
    opencode.interrupt(&gone).unwrap();

    let calls = oc.calls();
    assert_eq!(
        calls[0],
        ["api", "post", &format!("/api/session/{CREATED}/interrupt")]
    );
    assert_eq!(calls.len(), 2);
}

#[test]
fn telling_prompts_the_session_and_succeeds_on_one_that_is_gone() {
    let oc = StandIn::new(&[
        Reply::Ok("prompt-accepted.json"),
        Reply::Failed {
            stdout: "session-not-found.json",
            stderr: "session-not-found.stderr",
        },
    ]);
    let opencode = oc.opencode();
    let builder = Session {
        id: CREATED.into(),
        title: "#12".into(),
    };
    let gone = Session {
        id: "ses_doesnotexist".into(),
        title: "#13".into(),
    };

    opencode.tell(&builder, "Every check passed.").unwrap();
    opencode.tell(&gone, "Every check passed.").unwrap();

    let calls = oc.calls();
    assert_eq!(
        calls[0][..3],
        ["api", "post", &format!("/api/session/{CREATED}/prompt")]
    );
    assert_eq!(
        body(&calls[0]),
        serde_json::json!({ "text": "Every check passed." })
    );
    assert_eq!(calls.len(), 2);
}

#[test]
fn a_reply_that_is_not_a_session_is_refused() {
    let oc = StandIn::new(&[
        Reply::Ok("session-list-empty.json"),
        Reply::Ok("interrupted-idle.json"),
    ]);

    let error = oc
        .opencode()
        .start(Path::new("/w"), &brief(), None)
        .unwrap_err();

    assert!(error.is_permanent());
    assert_eq!(oc.calls().len(), 2);
}

#[test]
fn a_session_list_that_is_not_one_is_refused() {
    let oc = StandIn::new(&[Reply::Ok("interrupted-idle.json")]);

    let error = oc
        .opencode()
        .start(Path::new("/w"), &brief(), None)
        .unwrap_err();

    assert!(error.is_permanent());
    assert_eq!(oc.calls().len(), 1);
}

#[test]
fn it_is_the_build_s_agent_under_its_name() {
    let opencode = OpenCode::new();
    let registry = Registry::new(&[&opencode]);

    assert_eq!(opencode.name(), "opencode");
    assert!(registry.find_agent(None).is_ok());
    assert!(registry.find_agent(Some("opencode")).is_ok());
}
