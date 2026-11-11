//! A stand-in `gh`: the nth call prints the nth reply and exits as `gh`
//! would for its status, and records its arguments.

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test support: a fixture that breaks fails the test that uses it"
)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use atelier_github::GitHub;

static NEXT: AtomicU32 = AtomicU32::new(0);

const SCRIPT: &str = r#"#!/bin/sh
dir=$(dirname "$0")
n=$(( $(cat "$dir/count") + 1 ))
echo "$n" > "$dir/count"
for arg in "$@"; do printf '%s\n' "$arg"; done > "$dir/call-$n"
[ -f "$dir/reply-$n" ] && cat "$dir/reply-$n"
exit "$(cat "$dir/exit-$n" 2>/dev/null || echo 0)"
"#;

/// What the stand-in prints for one call.
#[derive(Clone, Copy)]
pub enum Reply {
    /// A captured reply, by its fixture's name; exits 1 unless it is a 2xx.
    Captured(&'static str),
    /// Nothing, and success, as `gh label create` prints when it works.
    Done,
    /// Nothing, and failure, as `gh` does when it cannot reach GitHub.
    Unreachable,
}

pub struct StandIn {
    dir: PathBuf,
}

impl StandIn {
    /// Replies in order.
    pub fn new(replies: &[Reply]) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("atelier-gh-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("count"), "0").unwrap();
        std::fs::write(dir.join("gh"), SCRIPT).unwrap();
        std::fs::set_permissions(dir.join("gh"), std::fs::Permissions::from_mode(0o755)).unwrap();

        for (index, reply) in replies.iter().enumerate() {
            let n = index + 1;
            let name = match reply {
                Reply::Captured(name) => *name,
                Reply::Done => {
                    std::fs::write(dir.join(format!("exit-{n}")), "0").unwrap();
                    continue;
                }
                Reply::Unreachable => {
                    std::fs::write(dir.join(format!("exit-{n}")), "1").unwrap();
                    continue;
                }
            };
            let text = std::fs::read_to_string(fixture(name)).unwrap();
            let ok = text.starts_with("HTTP/2.0 2");
            std::fs::write(dir.join(format!("reply-{n}")), text).unwrap();
            std::fs::write(dir.join(format!("exit-{n}")), if ok { "0" } else { "1" }).unwrap();
        }

        Self { dir }
    }

    pub fn github(&self) -> GitHub {
        GitHub::with_program(self.dir.join("gh"))
    }

    /// Each call's arguments, in order.
    pub fn calls(&self) -> Vec<Vec<String>> {
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

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/github/fixtures")
        .join(name)
}
