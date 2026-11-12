//! A sandbox for the real `atelier` binary: its own home, whose git config
//! sends `https://github.com/<owner>/<name>.git` to a real repository
//! under `<root>/github`, a real `origin` there for `StevenJPx2/demo`, your
//! checkout of it, and stand-ins for `gh` and `opencode` first on `PATH`.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test support: a sandbox that breaks fails the test that uses it"
)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;

static NEXT: AtomicU32 = AtomicU32::new(0);

pub const REPO: &str = "StevenJPx2/demo";
const URL: &str = "https://github.com/StevenJPx2/demo.git";

pub struct Sandbox {
    pub root: PathBuf,
    pub home: PathBuf,
    pub bin: PathBuf,
    pub checkout: PathBuf,
    seed: PathBuf,
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "atelier")
        .env("GIT_AUTHOR_EMAIL", "atelier@example.com")
        .env("GIT_COMMITTER_NAME", "atelier")
        .env("GIT_COMMITTER_EMAIL", "atelier@example.com")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The body of a reply captured from GitHub, without its status and headers.
fn captured_body(name: &str) -> String {
    let path = manifest()
        .join("../../plugins/github/tests/github/fixtures")
        .join(name);
    let text = fs::read_to_string(path).unwrap();

    text.split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap()
}

impl Sandbox {
    /// A sandbox whose GitHub holds `issues`, as `{"12": {"title", "state",
    /// "labels", "assignee"}}`, with the project already watched.
    pub fn new(issues: &Value) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("atelier-cli-{}-{id}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let sandbox = Self {
            home: root.join("home"),
            bin: root.join("bin"),
            checkout: root.join("demo"),
            seed: root.join("seed"),
            root,
        };

        sandbox.repositories();
        sandbox.stand_ins(issues);
        let added = sandbox.atelier(&["add", REPO]);
        assert!(
            added.status.success(),
            "{}",
            String::from_utf8_lossy(&added.stderr)
        );
        sandbox
    }

    fn repositories(&self) {
        let github = self.root.join("github");
        let origin = self.origin();
        fs::create_dir_all(&github).unwrap();
        fs::create_dir_all(&self.home).unwrap();
        fs::write(
            self.home.join(".gitconfig"),
            format!(
                "[url \"{}/\"]\n\tinsteadOf = https://github.com/\n",
                github.display()
            ),
        )
        .unwrap();

        let origin_path = origin.to_string_lossy();
        git(
            &self.root,
            &[
                "init",
                "--quiet",
                "--bare",
                "--initial-branch",
                "main",
                &origin_path,
            ],
        );
        git(&self.root, &["clone", "--quiet", &origin_path, "seed"]);
        git(
            &self.seed,
            &["commit", "--quiet", "--allow-empty", "-m", "base"],
        );
        git(&self.seed, &["push", "--quiet", "origin", "main"]);
        git(&self.root, &["clone", "--quiet", &origin_path, "demo"]);

        let rewrite = format!("url.{}.insteadOf", origin.display());
        git(&self.checkout, &["config", &rewrite, URL]);
        git(&self.checkout, &["remote", "set-url", "origin", URL]);
    }

    /// The real `origin` that `https://github.com/StevenJPx2/demo.git` reaches.
    pub fn origin(&self) -> PathBuf {
        self.root.join("github/StevenJPx2/demo.git")
    }

    /// atelier's own clone of the project.
    pub fn clone(&self) -> PathBuf {
        self.home
            .join(".local/share/atelier/clones/StevenJPx2/demo.git")
    }

    fn stand_ins(&self, issues: &Value) {
        let fixtures = self.bin.join("fixtures");
        let opencode = self.bin.join("opencode-fixtures");
        fs::create_dir_all(&fixtures).unwrap();
        fs::create_dir_all(&opencode).unwrap();
        fs::write(fixtures.join("issue-6.json"), captured_body("issue-6.txt")).unwrap();
        fs::copy(
            manifest().join("../../plugins/github/tests/github/fixtures/git-protocol-https.txt"),
            fixtures.join("git-protocol.txt"),
        )
        .unwrap();

        let captured = manifest().join("../../plugins/opencode/tests/opencode/fixtures");
        for name in [
            "session-created.json",
            "interrupted-idle.json",
            "session-not-found.json",
        ] {
            fs::copy(captured.join(name), opencode.join(name)).unwrap();
        }

        for name in ["gh", "opencode"] {
            fs::copy(
                manifest().join("tests/cli/stand_in").join(name),
                self.bin.join(name),
            )
            .unwrap();
            fs::set_permissions(self.bin.join(name), fs::Permissions::from_mode(0o755)).unwrap();
        }

        let world = serde_json::json!({ "issues": issues });
        fs::write(self.bin.join("world.json"), world.to_string()).unwrap();
    }

    /// Commits `text` as `.atelier/config.toml` on `origin`'s main.
    pub fn config(&self, text: &str) {
        self.config_on("main", text);
    }

    /// Commits `text` as `.atelier/config.toml` on `origin/<branch>`,
    /// making the branch from main if it is new.
    pub fn config_on(&self, branch: &str, text: &str) {
        git(&self.seed, &["switch", "--quiet", "main"]);
        if branch != "main" {
            git(&self.seed, &["switch", "--quiet", "-C", branch]);
        }
        fs::create_dir_all(self.seed.join(".atelier")).unwrap();
        fs::write(self.seed.join(".atelier/config.toml"), text).unwrap();
        git(&self.seed, &["add", ".atelier/config.toml"]);
        git(&self.seed, &["commit", "--quiet", "-m", "config"]);
        git(
            &self.seed,
            &["push", "--quiet", "--force", "origin", branch],
        );
        git(&self.seed, &["switch", "--quiet", "main"]);
    }

    /// What `atelier status` prints.
    pub fn status(&self) -> String {
        String::from_utf8(self.atelier(&["status"]).stdout).unwrap()
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let path = format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut command = Command::new(env!("CARGO_BIN_EXE_atelier"));
        command
            .args(args)
            .env("HOME", &self.home)
            .env("PATH", path)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_STATE_HOME")
            .env_remove("XDG_DATA_HOME");
        command
    }

    pub fn atelier(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// The daemon, appending to `daemon.log`; killed when the guard drops.
    pub fn daemon(&self) -> Daemon {
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("daemon.log"))
            .unwrap();
        let child = self
            .command(&["daemon"])
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .stdin(Stdio::null())
            .spawn()
            .unwrap();

        Daemon(child)
    }

    pub fn log(&self) -> String {
        fs::read_to_string(self.root.join("daemon.log")).unwrap_or_default()
    }

    pub fn state(&self) -> Value {
        let path = self.home.join(".local/state/atelier/state.json");

        fs::read_to_string(path).map_or(Value::Null, |text| serde_json::from_str(&text).unwrap())
    }

    /// The issue's phase in `state.json`, such as `building`, once its
    /// step's actions are all done.
    pub fn phase(&self, id: &str) -> Option<String> {
        let state = self.state();
        let issues = state["projects"][REPO]["issues"].as_array()?;
        let issue = issues.iter().find(|issue| issue["id"] == id)?;
        let settled = issue["pending"].as_array().is_some_and(Vec::is_empty);

        settled
            .then(|| issue["phase"]["name"].as_str().map(str::to_owned))
            .flatten()
    }

    /// Opens a draft pull request numbered `number` on `branch`, its checks'
    /// rollup in `checks`, such as `PENDING` or `SUCCESS`.
    pub fn open_pull_request(&self, branch: &str, number: u64, checks: &str) {
        let pull = serde_json::json!({ branch: {
            "number": number,
            "state": "OPEN",
            "isDraft": true,
            "commits": { "nodes": [{ "commit": { "statusCheckRollup": { "state": checks } } }] }
        }});
        let temporary = self.bin.join("pulls.json.tmp");
        fs::write(&temporary, pull.to_string()).unwrap();
        fs::rename(temporary, self.bin.join("pulls.json")).unwrap();
    }

    /// Sets issue `id`'s labels on the stand-in GitHub, as a person would.
    pub fn set_labels(&self, id: &str, labels: &[&str]) {
        self.set(id, "labels", serde_json::json!(labels));
    }

    /// Sets a field of issue `id` on the stand-in GitHub, such as its state.
    pub fn set(&self, id: &str, field: &str, value: Value) {
        let mut world = self.world();
        world["issues"][id][field] = value;
        let temporary = self.bin.join("world.json.tmp");
        fs::write(&temporary, world.to_string()).unwrap();
        fs::rename(temporary, self.bin.join("world.json")).unwrap();
    }

    pub fn world(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.bin.join("world.json")).unwrap()).unwrap()
    }

    /// Each call the stand-in `gh` received.
    pub fn gh_calls(&self) -> Vec<Vec<String>> {
        let text = fs::read_to_string(self.bin.join("calls.jsonl")).unwrap_or_default();

        text.lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    pub fn opencode_calls(&self) -> Vec<String> {
        let text = fs::read_to_string(self.bin.join("opencode-calls.log")).unwrap_or_default();

        text.lines().map(str::to_owned).collect()
    }

    /// Waits up to 30 seconds for `done`.
    pub fn wait(&self, what: &str, done: impl Fn(&Self) -> bool) {
        let started = Instant::now();

        while !done(self) {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "waited 30 s for {what}; log:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub struct Daemon(pub Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
