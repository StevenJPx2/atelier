# Slice 3: pipelines made of steps

**Status: In progress.** The plan is
[#43](https://github.com/StevenJPx2/atelier/issues/43), slice 3 of
[#1](https://github.com/StevenJPx2/atelier/issues/1). The `sourcefed` trigger
follows as slice 4 ([#42](https://github.com/StevenJPx2/atelier/issues/42)).
Pipelines run on [rungs](https://github.com/StevenJPx2/rungs) `v0.1.0`, and
an earlier rc's `state.json` is read into runs of `build`. Agents settle
their steps with `atelier finish`, and a person stops a run with
`atelier stop`. Left: a trigger mark for each pipeline; until then the build
mark starts `build`.

## You can

Give a project any number of pipelines, each a set of steps written in its
config. A step can be one a plugin provides, an agent with your own prompt, or a
shell command. Each step says which earlier outcomes it waits for, and steps
whose needs are met together run at the same time. Today's loop, from an issue
to a merged pull request, becomes the `build` pipeline, written in the same
terms.

For example, atelier adds `security-audit`. Labelling an issue
`atelier:security-audit` makes a worktree. Then `cargo deny` and an agent
following the audit prompt run side by side. If either finds something, a second
agent fixes it in a pull request. If both come back clean, the run is Done and
the issue closes with what was checked.

```toml
# atelier's .atelier/config.toml
[[pipeline.security-audit.step]]
id = "worktree"
uses = "git.worktree"

[[pipeline.security-audit.step]]
id = "deps"
uses = "atelier.command"
run = "cargo deny check advisories"
after = ["worktree"]

[[pipeline.security-audit.step]]
id = "audit"
uses = "agent.session"
prompt = ".atelier/prompts/security-audit.md"
outcomes = ["found", "clean"]
after = ["worktree"]

[[pipeline.security-audit.step]]
id = "fix"
uses = "agent.session"
prompt = ".atelier/prompts/fix-findings.md"
outcomes = ["opened"]
after_any = { audit = "found", deps = "failed" }
```

```mermaid
flowchart LR
    W["worktree<br/>git.worktree"] --> D["deps<br/>atelier.command"]
    W --> A["audit<br/>agent.session"]
    D -- "failed" --> X["fix<br/>agent.session"]
    A -- "found" --> X
    D -- "passed" --> Done["Done, when nothing<br/>else can run"]
    A -- "clean" --> Done
```

## What it builds

### Steps

Every step has the same shape:

| Key           | Meaning                                                                                                               | Default            |
| ------------- | --------------------------------------------------------------------------------------------------------------------- | ------------------ |
| `id`          | Its name in this pipeline                                                                                             | required           |
| `uses`        | The step kind, `<namespace>.<kind>`                                                                                   | required           |
| `after`       | Steps it waits for. A list means each of them passed; a table names the outcome each must have, or a list of outcomes | the step before it |
| `after_any`   | As `after`, but one is enough                                                                                         | none               |
| `outcomes`    | The outcomes it can settle with, besides `failed`                                                                     | the kind's own     |
| `rounds`      | How many times it may fail before the failure is final                                                                | 1                  |
| `restart`     | A push sends the run back here                                                                                        | false              |
| anything else | The kind's own settings, such as `prompt` or `run`                                                                    |                    |

- **`uses` names a role or a plugin.** `agent.session`, `forge.checks` and
  `tracker.comment` go to whichever plugin fills that role in the project, so a
  committed config works with any agent or forge. `opencode.session` or
  `github.checks` name one plugin, for a step that only works there. `atelier.*`
  are core's own kinds.
- **A step waits until every step it names has settled or been skipped,** then
  runs if its needs hold and is skipped if they don't. Steps that become ready
  together start together.
- **A failed step under its `rounds` starts again.** Its final failure goes to
  any step that names `failed` in its needs; if none does, the run blocks.
- **A run is Done** when no step is running and none can start. It closes the
  work item with a comment listing each step's outcome.
- **Resident steps,** an agent's session, stay alive after they settle, so later
  steps can talk to them, as `forge.ready` tells the builder every check passed.
  Every way out of a run interrupts them and tells them why.
- **Config is checked when it loads.** An unknown kind, a cycle, `after` naming
  a later or unknown step, an outcome the step can't settle with, or more than 8
  steps each refuse the pipeline with its name and the reason:
  `security-audit: no step kind "forge.reviews"; github has checks, ready, merge`.

#### Structure

`Pipeline::read` reads one pipeline from its steps' settings, each a
`[[pipeline.<name>.step]]` table, and checks it against the kinds the project
can use. A step with neither `after` nor `after_any` waits for the step before
it to pass; `after = []` waits for nothing. Each step's other keys go to its
kind's plugin, which checks them and returns them as the step runs with them.
A pipeline's name, each id and each outcome take letters, digits, `-` and `_`;
`rounds` is 1 to 255.

```rust
pub const MAX_STEPS: usize = 8;
pub const PASSED: &str = "passed";
pub const FAILED: &str = "failed";

pub struct Pipeline {
    name: String,
    steps: Vec<StepSpec>,
}

impl Pipeline {
    pub fn read(
        name: &str,
        steps: &[Settings],
        kinds: &Kinds<'_>,
        tree: &dyn Tree,
    ) -> Result<Self, PipelineError>;
    pub fn name(&self) -> &str;
    pub fn steps(&self) -> &[StepSpec];
    pub fn step(&self, id: &str) -> Option<&StepSpec>;
}

pub struct PipelineError {
    pub pipeline: String,
    pub problem: String,
}
```

A checked step keeps the kind as written and the plugin it resolved to, so
`atelier status` shows `forge.checks` as `github.checks`.

```rust
pub struct StepSpec {
    pub id: String,
    pub uses: String,
    pub plugin: &'static str,
    pub kind: String,
    pub needs: Needs,
    pub outcomes: Vec<String>,
    pub rounds: u8,
    pub restart: bool,
    pub resident: bool,
    pub settings: Settings,
}

impl StepSpec {
    pub fn resolved(&self) -> String;
}

pub struct Needs {
    pub all: Vec<Need>,
    pub any: Vec<Need>,
}

pub struct Need {
    pub step: String,
    pub outcomes: Vec<String>,
}
```

`Kinds` holds each namespace and the plugin behind it. The registry puts each
plugin's kinds under its own name, and the project's agent, forge and only
tracker under `agent`, `forge` and `tracker`; with no agent named, the
build's only agent fills the role.

```rust
pub struct Kinds<'a> {
    namespaces: Vec<Namespace<'a>>,
}

impl<'a> Kinds<'a> {
    pub fn with(
        self,
        namespace: &str,
        plugin: &'static str,
        step: &'a dyn Step,
    ) -> Self;
}

impl<'a> Registry<'a> {
    pub fn step(&mut self, step: &'a dyn Step);
    pub fn kinds(
        &self,
        agent: Option<&str>,
        forge: &str,
        trackers: &[&str],
    ) -> Kinds<'a>;
}
```

A step's settings are read with `Settings`' typed getters, which name the key
of any value of the wrong type, such as `checks.rounds: must be a number`.

```rust
impl Settings {
    pub fn get(&self, key: &str) -> Option<&Setting>;
    pub fn number(&self, path: &str, key: &str)
        -> Result<Option<i64>, SettingsError>;
    pub fn flag(&self, path: &str, key: &str)
        -> Result<Option<bool>, SettingsError>;
    pub fn without(&self, keys: &[&str]) -> Self;
}
```

### Step kinds

| Kind              | From                        | Starts by                                                                                                                                                  | Settles                                                                   |
| ----------------- | --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `git.worktree`    | `git`                       | creating the run's worktree from `origin/<base>`                                                                                                           | `passed` when it exists                                                   |
| `agent.session`   | the agent role (`opencode`) | starting a resident session with `prompt`, read from `origin/<base>`, or `brief = "builder"`, core's builder contract with the tracker's and forge's parts | when the agent runs `atelier finish <outcome> "<why>"`                    |
| `atelier.command` | core                        | running `run` in the worktree, with a time limit                                                                                                           | `passed` on exit 0, otherwise `failed`                                    |
| `forge.checks`    | the forge role (`github`)   | nothing: CI runs on each push                                                                                                                              | `passed` when every check on the head passed, `failed` for a failed round |
| `forge.ready`     | the forge role              | telling the resident agent every check passed                                                                                                              | `passed` when the pull request is marked ready                            |
| `forge.merge`     | the forge role              | nothing: a person merges                                                                                                                                   | `merged`, or `failed` when closed unmerged                                |

A plugin adds kinds through the new `Step` role:

```rust
pub trait Step {
    /// The kinds this plugin offers, such as "checks" for "github.checks".
    fn kinds(&self) -> Vec<Kind>; // name, outcomes, resident

    /// Checks the step's own settings when the config loads; `tree` is the
    /// project at origin/<base>, where `prompt` names a file.
    fn configure(
        &self,
        kind: &str,
        settings: &Settings,
        tree: &dyn Tree,
    ) -> Result<Settings, SettingsError>;

    /// Starts the step for a run, or starts it again after a failed round,
    /// and returns at once: a worktree, a session, or words for the
    /// resident steps.
    fn start(&self, run: &RunContext<'_>, step: &StepSpec)
        -> Result<Started, PluginError>;

    /// Releases resources kept in the step's folder on every run exit;
    /// safe when no start made anything, or cleanup already ran.
    fn stop(&self, folder: &Path) -> Result<(), PluginError>;

    /// What a poll sees: running, settled, a failed round, or, for a
    /// settled step, that it runs again.
    fn observe(&self, run: &RunContext<'_>, step: &StepSpec)
        -> Result<Observed, PluginError>;
}
```

A future `critic` or `recording` plugin is one more set of kinds,
`critic.review` or `recording.record`, with nothing in core about it.

Files: `crates/core/src/pipeline/` (`Pipeline`, `StepSpec`, `Needs`, the config
check), `crates/core/src/roles/step.rs`, `crates/cli/src/command_step/`, and
each plugin's `steps.rs`.

#### Structure

A kind's outcomes are its own, which a step cannot change, or named by each
step, as an agent's are; a step that names none settles `passed`. Every step
can also settle `failed`.

```rust
pub struct Kind {
    pub name: &'static str,
    pub outcomes: Outcomes,
    pub resident: bool,
}

pub enum Outcomes {
    Own(&'static [&'static str]),
    Named,
}
```

`git` is a plugin of its own, offering `worktree`. `atelier` is the CLI's
plugin of core's own kinds, offering `command`, which takes `run` and
`timeout`, its limit in seconds: 600 unless set, at most a day. GitHub offers `checks`,
`ready` and `merge`, none with settings. OpenCode offers `session`, which
takes `prompt`, a file on the base branch whose text it keeps under `text`,
or `brief = "builder"`. A prompted session's title includes its step id, so
two agent steps in the worktree keep separate sessions. A prompt is read
from `origin/<base>` through a
`BranchTree`, listed once and each file read when asked.

```rust
impl Plugin for Git {}
impl Step for Git {}
impl Step for GitHub {}
impl Step for OpenCode {}
impl Step for Atelier {}

impl Repository {
    pub fn tree(&self, branch: &str) -> Result<BranchTree, PluginError>;
}

pub struct BranchTree {
    repository: Repository,
    branch: String,
    files: Vec<String>,
}

impl Tree for BranchTree {}
```

A command runs in the background, in its own process group, so a poll never
waits on it and a daemon restarted while it runs leaves it running. A start
empties the step's folder and starts the command there; a later poll reads how
it ended. It settles `passed` on exit 0, and `failed` on any other exit, past
its limit, which kills its whole group, or when it stopped without an exit
status; each says why, with the last 20 lines it printed. `ATELIER_BASE` is
`origin/<base>`, as for the gates.

```rust
pub struct Outcome {
    pub name: String,
    pub why: String,
}

pub fn start(
    folder: &Path,
    worktree: &Path,
    base: &str,
    settings: &Settings,
) -> Result<(), PluginError>;

pub fn observe(folder: &Path, settings: &Settings)
    -> Result<Option<Outcome>, PluginError>;
```

### The `build` pipeline

Built in, and written in the same terms. A project that writes
`[pipeline.build]` replaces it:

```toml
[[pipeline.build.step]]
id = "worktree"
uses = "git.worktree"

[[pipeline.build.step]]
id = "builder"
uses = "agent.session"
brief = "builder"
# `atelier finish opened` once the draft pull request is open
outcomes = ["opened", "nothing"]

[[pipeline.build.step]]
id = "checks"
uses = "forge.checks"
after = { builder = "opened" }
rounds = 5
restart = true

[[pipeline.build.step]]
id = "ready"
uses = "forge.ready"

[[pipeline.build.step]]
id = "merge"
uses = "forge.merge"
```

With a critic and a recording, run side by side once the checks pass:

```toml
[[pipeline.build.step]]
id = "critic"
uses = "critic.review"
after = ["checks"]
rounds = 2

[[pipeline.build.step]]
id = "recording"
uses = "recording.record"
after = ["checks"]

[[pipeline.build.step]]
id = "ready"
uses = "forge.ready"
after = ["critic", "recording"]
```

`builder` settling `nothing` leaves no step able to run, so the run is Done: the
issue closes as not planned, with the builder's reason.

#### Structure

The built-in `build` is `crates/cli/src/build.toml`, read as any config is.
The config keeps each pipeline as written, the built-in `build` first unless
`[pipeline.build]` replaces it, then each written pipeline by name. Each poll
and `atelier status` check them against the project's plugins and its files
on `origin/<base>`; a pipeline atelier cannot run refuses the project.

```rust
pub struct Written {
    pub name: String,
    pub steps: Vec<Settings>,
    pub origin: Origin,
}

pub struct Resolved {
    pub pipelines: Vec<Written>,
    …
}

impl Build {
    pub fn new(paths: &Paths) -> Self;
    pub const fn git(&self) -> &Git;
    pub fn pipelines(&self, resolved: &Resolved, tree: &dyn Tree)
        -> Result<Vec<Pipeline>>;
}
```

`atelier status` shows each pipeline, where it came from, and each step's
kind with the plugin a role's kind resolved to:

```text
  pipeline    build                    found
    worktree  git.worktree
    builder   agent.session → opencode.session
    checks    forge.checks → github.checks
    ready     forge.ready → github.ready
    merge     forge.merge → github.merge
```

### Triggers and runs

- **A tracker's mark starts a run.** An item carrying `atelier:<pipeline>`
  starts that pipeline, so `atelier:build` keeps its meaning. `atelier init`
  creates a label for each pipeline. In this slice the tracker's listing finds
  them, as today; from slice 4 the trigger role does, with sourcefed's events
  ([#42](https://github.com/StevenJPx2/atelier/issues/42)), and the listing only
  reconciles.
- **A run** is one pass of a pipeline for one item, such as `build github#12`.
  It replaces "an issue" in `state.json` and `atelier status`. An item has at
  most one working run at a time.

### The machine

The step logic is [rungs](https://github.com/StevenJPx2/rungs)
([its plan](https://github.com/StevenJPx2/rungs/issues/1)), a crate shared with
Chauffeur: no I/O, no dependencies, `no_std`, proven with Kani. Core lowers a
pipeline to a `rungs::Plan`, saves each run's `rungs::Progress` in `state.json`,
and turns what rungs reports into actions:

```rust
// rungs::Plan: positions, outcomes, needs, attempts
let plan = pipeline.plan();

// worktree passed: deps and audit start side by side
let settled = Event::Settled { step: 0, outcome: Outcome(0) };
let (next, actions) = step(run, settled, plan);
// → Running, [Start(1), Start(2)]

// a push restarts checks and the steps after it
let (next, actions) = step(run, Event::Pushed { step: 2 }, plan);
// rungs::State::Over { unhandled: Some(i) } → Blocked(Failed(i))
```

```mermaid
stateDiagram-v2
    direction LR
    [*] --> Pending
    Pending --> Running: its needs hold
    Pending --> Skipped: its needs cannot hold
    Running --> Running: failed, under its rounds
    Running --> Settled: an outcome
    Running --> Pending: a push, at or after a restart point
    Settled --> Pending: a push, at or after a restart point
```

- Each step is Pending, Running with its failed rounds, Settled with its
  outcome, or Skipped. The run is Claiming, Running, Done, Blocked at a step, or
  Stopped.
- A push resets each started restart step, and every step that depends on it, to
  Pending; failed rounds are kept, so CI rounds still add up across pushes.
- Stopping, blocking and Done each remove the working mark, interrupt every
  resident step that started, cancel background commands, and remove the
  worktree. Each step's plugin cleans up its own folder; the record keeps
  which plugin started it even if its pipeline is removed. Each pass has a
  separate folder under `steps/<item>/<pipeline>/<generation>/<step>`.

rungs proves its own promises
([rungs#1](https://github.com/StevenJPx2/rungs/issues/1)), for every plan of up
to four steps: a step starts only when every step it names has settled or been
skipped and its needs hold, it is skipped only when they cannot, a step fails
at most its attempts, a restart keeps failed attempts, and `state` is Over
only when no step runs and none can start. Core's two Kani harnesses prove
what core adds over it:

- For every answer rungs can give, the run follows it: a step starts exactly
  when rungs started it, the run goes on exactly while rungs says it runs,
  Done comes only when it is over with nothing unhandled, and a final failure
  no step waits for blocks the run at that step. Each way out interrupts the
  resident steps and removes the working mark and the worktree.
- For every run and every event rungs plays no part in, an event that does
  not apply changes nothing, a claim comes only from a request and starts
  fresh, and every way out of a working run removes the working mark, and of
  a running one interrupts its resident steps and removes the worktree.

Tests with pipelines up to the limit of eight steps cover the rest, including
the recorded event sequences of slices 1 and 2 on the written-out `build`.

Files: `crates/core/src/machine/`, rewritten over rungs, with `proofs.rs`;
`crates/core/src/pipeline/lower.rs`; `crates/core/src/forge_steps.rs`; and
`crates/core/src/poll/`. rungs itself is built first, in its own repository,
and tagged `v0.1.0`.

#### Structure

A checked pipeline keeps its steps as a rungs plan: each outcome by its place
in the step's `outcomes`, `failed` as `Outcome::FAILED`, `rounds` as
attempts, and needs by position.

```rust
impl Pipeline {
    pub const fn plan(&self) -> &rungs::Plan;
}

impl StepSpec {
    pub fn outcome(&self, name: &str) -> Option<Outcome>;
    pub fn outcome_name(&self, outcome: Outcome) -> Option<&str>;
}
```

A run is its phase and its steps' progress. `step` is pure and total: an
event that does not apply leaves the run and names nothing. A request claims
with fresh progress; a claim that fails also takes the build mark, so it is
not requested again until a person asks. A failed round under a step's
`rounds` names `Start` again; `last` makes it final at once, as a pull
request closed without merging does. `Pushed` restarts only a settled step.

```rust
pub struct Run {
    pub phase: Phase,
    pub progress: Progress,
}

pub enum Phase { Untracked, Claiming, Running, Done, Blocked(Reason), Stopped }

pub enum Reason { Claim, Start(u8), Failed(u8), Changed, LostTrack }

pub enum Event {
    Requested,
    Claimed,
    ClaimFailed,
    Settled { step: u8, outcome: Outcome },
    Failed { step: u8, last: bool },
    StartFailed { step: u8 },
    Pushed { step: u8 },
    Changed,
    Stop,
    Unmarked,
    IssueClosed,
    LostTrack,
}

pub enum Action {
    Mark(Mark),
    Unmark(Mark),
    Start(u8),
    Announce(u8),
    Interrupt,
    Report,
    CloseIssue,
    RemoveWorktree,
    Comment(Reason),
}

pub fn step(run: Run, event: Event, plan: &rungs::Plan) -> (Run, Vec<Action>);
```

A step's plugin sees its run through a `RunContext`: the item, branch,
worktree, its own folder, the builder's brief, its last session, the pull
request the poll saw, and its failed rounds. A start returns what the run
keeps, and an observation what the step did.

```rust
pub struct RunContext<'a> {
    pub repo: &'a Repo,
    pub local: &'a Path,
    pub base: &'a str,
    pub issue: &'a Issue,
    pub name: &'a str,
    pub branch: &'a str,
    pub worktree: Option<&'a Path>,
    pub folder: &'a Path,
    pub brief: &'a Brief,
    pub session: Option<&'a Session>,
    pub pull_request: Option<PullRequest>,
    pub failed: u8,
    pub settled: bool,
}

pub enum Started { Nothing, Worktree(PathBuf), Session(Session), Tell(String) }

pub enum Observed {
    Running,
    Settled(Outcome),
    Failed { why: String, last: bool },
    Restarted,
}
```

Every forge's kinds read the pull request the forge reports, so
`forge.checks` means the same on any forge. `checks` fails a round for each
head commit with a failed check and passes when every check passed; once
settled, a pending or newly failed head makes it run again. `ready` tells
the resident steps every check passed and passes once the pull request is
ready; `merge` settles `merged`. A pull request closed without merging fails
each of them for good. The builder's step settles `opened` once a pull
request is on its branch.

```rust
pub fn forge_kinds() -> Vec<Kind>;
pub fn start_forge_step(kind: &str, run: &RunContext<'_>)
    -> Result<Started, PluginError>;
pub fn observe_forge_step(kind: &str, run: &RunContext<'_>)
    -> Result<Observed, PluginError>;
pub fn observe_builder(run: &RunContext<'_>) -> Observed;
```

`state.json` keeps each item's run: its pipeline, phase and progress, and
for each step, in the pipeline's order, its session and why it last settled.
A running run whose steps differ from its pipeline's, or whose progress
the plan refuses, blocks as `Changed`. A request takes the pipeline's steps
afresh and keeps each step's last session, which its next start replaces.

```rust
pub struct Project {
    pub repo: Repo,
    pub local: PathBuf,
    pub base: String,
    pub forge: String,
    pub pipelines: Vec<Pipeline>,
    pub steps: PathBuf,
}

pub struct Record {
    pub generation: u64,
    pub pipeline: String,
    pub phase: Phase,
    pub progress: Progress,
    pub steps: Vec<StepRecord>,
    pub title: String,
    pub branch: String,
    pub worktree: Option<PathBuf>,
    pub pull_request: Option<u64>,
    pub pending: Vec<Action>,
}

pub struct StepRecord {
    pub id: String,
    pub plugin: String,
    pub session: Option<Session>,
    pub why: Option<String>,
}

pub struct Move {
    pub key: IssueKey,
    pub pipeline: String,
    pub from: Phase,
    pub to: Phase,
    pub what: String,
}
```

### `atelier finish` and `atelier stop`

```sh
# run by an agent step, in its worktree
atelier finish found "two advisories in toml_edit"
# run by a person
atelier stop "security-audit github#40"
```

Each finds the run by its worktree or key and leaves a request for the project's
thread, which reads it on its next poll. The daemon stays the only writer of
`state.json`. An agent's permissions allow `atelier finish *`, and `finish`
refuses an outcome the step doesn't declare. `finish` infers the step when
exactly one running agent step declares the outcome; if several do, it
refuses with their ids and requires `--step <id>`. Each agent step's prompt
includes its explicit finish command.

Files: `crates/cli/src/requests/`, `finish.rs`, `stop.rs`, and
`crates/core/src/poll/request.rs`.

#### Structure

A request names the run's generation, so one left from an older pass cannot
affect a fresh run. A finish also names the running agent step's session and
failed-round count; a stale request or a replay changes nothing. A stop
preempts finishes and pending starts and removes the trigger mark, so the
same request cannot start the run again.

```rust
pub struct Request {
    pub generation: u64,
    pub command: Command,
}

pub enum Command {
    Finish {
        step: String,
        outcome: Outcome,
        session: String,
        failed: u8,
    },
    Stop,
}

pub struct Observation {
    pub issues: BTreeMap<IssueKey, Watched>,
    pub requests: BTreeMap<IssueKey, Vec<Request>>,
    pub etags: BTreeMap<(String, Mark), String>,
}
```

Each project's queue has 128 slots under
`~/.local/state/atelier/requests/<owner>/<repo>/`. A temporary file reserves
a slot before it is written and renamed; a reader sees only ready files. A
full queue refuses another request without waiting. Each request is at most
8 KiB, and a finish reason at most 4096 bytes. The worker acknowledges only
after every action succeeds and its effects are saved, so a crash between
acting and acknowledgement replays safely.

```rust
pub fn submit(folder: &Path, key: &IssueKey, request: &Request) -> Result<()>;
pub fn read(folder: &Path) -> Result<Vec<Queued>>;
pub fn acknowledge(queued: &[Queued]) -> Result<()>;
```

Finish finds the working run whose worktree contains its path; stop finds a
pipeline/item key across the watched projects, with `--repo owner/name`
when several match. The builder's session title remains the issue title;
each agent step's prompt includes its outcomes and an explicit finish
command naming its own step.

```rust
pub struct Target {
    pub repo: String,
    pub key: IssueKey,
    pub record: Record,
}

pub fn in_worktree(paths: &Paths, path: &Path) -> Result<Target>;
pub fn by_key(paths: &Paths, selector: &str, repo: Option<&str>) -> Result<Target>;
```

### State from an older rc

`state.json` keeps each run's pipeline and every step's status. The new rc reads
an older file into the written-out `build`: Building is `builder` running; Open
is `builder` settled `opened` and `checks` running; Green is `ready` running;
and Ready is `merge` running.

Files: `crates/cli/src/state/`.

#### Structure

An older file's `issues` are read beside `runs` and never written back, so
the first save after the upgrade writes each as a run. Each issue keeps its
branch, worktree, pull request and pending actions, and its builder's
session on `builder`; a stage, which no plugin ran, reads as the checks it
followed. Done has every step settled; Blocked and Stopped start fresh when
requested again.

```rust
struct ProjectFile {
    runs: Vec<RunFile>,
    issues: Vec<older::IssueFile>,
    etags: Vec<EtagFile>,
}

impl older::IssueFile {
    pub fn into_record(self) -> (IssueKey, Record);
}
```

### Verification

- Kani as above, and the config check's refusals, one test each.
- The recorded event sequences of slices 1 and 2 on the written-out `build`,
  against an in-memory world, with a stop after any save resuming to the same
  end.

```text
for all started = which of the four steps rungs started
for all state   = Running, or Over with no step or any one unhandled

run, actions = conclude(started, state)

assert actions start step i  ==  started[i]
assert state is Running      implies run is Running, no mark removed
assert state is Over(none)   implies run is Done, and it interrupts,
                                     reports, closes, unmarks, removes
assert state is Over(i)      implies run is Blocked at i, and it
                                     interrupts, marks, unmarks, removes

for all run   = every phase, every progress of up to four steps
for all event = every event rungs plays no part in

next, actions = step(run, event)

assert next == run                       implies actions is empty
assert next is Claiming, run was not     implies event is Requested,
                                         progress is fresh
assert run was working, next is not      implies the working mark goes
assert run was Running, next is not      implies it interrupts and
                                         removes the worktree
```

- `atelier.command`: a passing command, a failing one, and one past its time
  limit.
- `atelier finish` and `atelier stop` against a running daemon in a temporary
  state folder.
- An older `state.json` read by the new daemon, with each run where it was.

## End to end

On the installed rc, on atelier:

1. **`build`, written out:** a small real issue goes from worktree to merge, as
   in slice 1.
2. **Parallel steps and outcomes:** `security-audit` on an issue. `deps` and
   `audit` start in the same poll. One run finds something real and `fix` opens
   a pull request; a later run with nothing left skips `fix` and closes the
   issue with each step's outcome.
3. **A plugin step by role:** `forge.checks` resolves to `github.checks`, as
   `atelier status` shows.
4. **The upgrade:** the daemon is upgraded from the previous rc while a `build`
   run is open, and the run continues.
5. **Stop:** `atelier stop` on a running audit interrupts both agents within a
   poll.

## Dependencies

| Crate         | Dependency                   | Why                                                                        |
| ------------- | ---------------------------- | -------------------------------------------------------------------------- |
| `crates/core` | `rungs`, by git tag `v0.1.0` | The step machine, shared with Chauffeur; it has no dependencies of its own |

The config is read with the CLI's `toml`.
