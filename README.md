# atelier

**Status: slice 2 implemented, `0.1.0-rc.7`.** Install with
`brew install StevenJPx2/taps/atelier`, then `atelier init`, `atelier add` and
`brew services start atelier`: a labelled issue becomes a builder and a ready
pull request, and a project's gates, listed as commands, run as one check each
in GitHub's CI and fix each builder commit. Language plugins, the critic and
recordings are the approved design below, planned for later slices. See the
[slice index](docs/plans/README.md), or read every page as a site at
[atelier-docs.stevenjpx2.workers.dev](https://atelier-docs.stevenjpx2.workers.dev).

A personal software factory. You plan a feature as a GitHub issue and add the
`atelier:build` label. atelier starts a builder agent in its own worktree, which
opens a pull request and works until every check passes, a critic agent finds
nothing blocking, and a recording shows the feature working. Then the pull request
is yours to review.

```sh
atelier init                         # in a project: labels and .atelier/config.toml
atelier add StevenJPx2/chauffeur     # watch it
brew services start atelier          # keep its daemon running
gh issue edit 12 --add-label atelier:build
```

## How a feature moves

```mermaid
sequenceDiagram
    participant You
    participant Atelier as atelier daemon
    participant Builder as Builder agent
    participant PR as Pull request and CI
    participant Critic as Critic agent
    You->>Atelier: label issue atelier:build
    Atelier->>Builder: worktree and session
    Builder->>PR: draft pull request
    PR-->>Builder: failed check, via sourcefed
    Builder->>PR: fix and push
    PR-->>Atelier: every check passes
    Atelier->>Critic: every check passed: review it
    Critic->>PR: findings with evidence
    Builder->>PR: fix and record
    Atelier->>Builder: every check and stage passed
    Builder->>PR: mark ready
    PR-->>You: ready for review
```

The checks decide when the work is done. The agent does not judge it, and no
rulebook or skill has to be followed: a builder knows its issue, the project's
`AGENTS.md` and the contract in
[the builder's brief](https://github.com/StevenJPx2/atelier/issues/1#the-builders-brief).

## A core and its plugins

The core is the issue machine and the agent orchestrator: it sees issues, decides
what happens to each (claim, start, stop, block, close), and starts an agent as the
project's config says. It knows no programming language, issue tracker, forge or
agent. Plugins do, each a Rust crate in this repository, compiled into the one
`atelier` binary. A plugin is generic; it registers the roles it fills:
`plugins/github` is both a tracker and the forge. A project has one forge and one
agent, and any number of trackers, such as GitHub and Jira together, each with its
own filter.

```mermaid
flowchart TB
    CLI["crates/cli<br/>atelier binary"] --> GitHub["plugins/github<br/>Tracker, Forge"]
    CLI --> OpenCode["plugins/opencode<br/>Agent"]
    CLI --> Langs["plugins/rust, tla, typescript<br/>Checks"]
    CLI --> Git["crates/git<br/>Worktrees"]
    GitHub --> Core["crates/core<br/>issue machine, traits"]
    OpenCode --> Core
    Langs --> Core
    Git --> Core
```

| Role    | Does                                     | Plugins                     |
| ------- | ---------------------------------------- | --------------------------- |
| Tracker | Finds, marks and closes issues           | `github`                    |
| Forge   | Pull requests, checks, how CI runs gates | `github`                    |
| Agent   | Starts and stops builders and critics    | `opencode`                  |
| Checks  | Finds its roots; names gates and fixes   | `rust`, `tla`, `typescript` |
| Stage   | Runs after every check, before ready     | `critic`, `recording`       |

Every plugin lives in this repository and ships in the release; atelier loads no
code at run time.

## Gates

atelier finds a project's languages, and where each lives, by itself. Each language
plugin finds its roots, the directories its gates run in, so one repository can
hold Rust in one directory and TLA+ specs in another, each with its own gates.
Checks plugins name the gates; the forge plugin defines how its CI runs them.

```mermaid
flowchart LR
    Lang["Checks plugins<br/>roots and their gates"] --> Forge["Forge plugin<br/>writes its CI"]
    Forge --> CI["Forge CI<br/>one job per gate"]
    CI -- "atelier check --gate" --> Check["the gate's command"]
    Check -- "pass or fail" --> PR["a check on<br/>the pull request"]
```

```text
.      rust/clippy     pass    21 s
.      rust/mutants    pass   130 s   14 mutants in the diff, 14 caught
specs  tla/tlc         pass     3 s   tick.tla
```

Before each of the builder's commits, a hook atelier puts in its worktree runs
`atelier fix`: formatting and lint fixes a tool can make land in the same commit.
On a pull request, CI runs the gates of each root the diff touches. The builder
never runs them: a failed gate reaches it as a failed check, through sourcefed.

The gates follow the code-discipline policy, one check each:

- **rust** ([#2](https://github.com/StevenJPx2/atelier/issues/2)): a root is a
  Cargo workspace or a crate outside one. fmt, Clippy with `-D warnings`,
  nextest, doc tests, rustdoc with `-D warnings`, cargo-deny, cargo-mutants on
  the pull request's diff (`--in-diff`), and Kani when a crate has proofs.
- **tla** ([#3](https://github.com/StevenJPx2/atelier/issues/3)): a root is a
  directory with `*.tla` specs. TLC on each, in a project of any language.
- **typescript** ([#4](https://github.com/StevenJPx2/atelier/issues/4)): a root is
  an npm, Bun or pnpm workspace, or a package outside one. oxfmt, Oxlint with the
  vendored anti-slop plugin and no warnings, `tsgo -b`, Effect diagnostics when
  it uses Effect, its tests, `fallow audit`, and the package manager's audit,
  after a frozen install.

## Adding a step

Every step between a commit and a ready pull request comes from a plugin, and each
project picks the ones it wants: fixes run in a commit hook, checks run in CI, and
the steps of a pipeline run once their needs hold. Language plugins are found;
other checks are named in the config. Chauffeur has a docs site, so it adds the
`docs` check, which builds the site and fails on a dead link or a diagram that
does not render; atelier, with no site, leaves it out:

```toml
# Chauffeur's .atelier/config.toml
base = "v2"
checks = ["docs"]
```

The loop from an issue to a merged pull request is the built-in `build`
pipeline: worktree, builder, checks, ready, merge. A project that writes
`[pipeline.build]` replaces it, with its own steps and rounds; a new step is a
kind a plugin offers, and core runs every step by its needs, knowing none by
name. `atelier status` shows each pipeline with the plugin behind each step.

## An issue's states

Labels hold the state, so an issue shows where its feature is.

```mermaid
stateDiagram-v2
    [*] --> Queued: you add atelier:build
    Queued --> Working: claimed, builder started
    Queued --> Blocked: the start fails
    Working --> Done: pull request merged
    Working --> Blocked: a limit is reached
    Working --> Stopped: you remove atelier:working
    Blocked --> Queued: you add atelier:build
    Stopped --> Queued: you add atelier:build
    Done --> [*]
```

| Label             | Set by  | Means                                            |
| ----------------- | ------- | ------------------------------------------------ |
| `atelier:build`   | You     | Start a builder on this issue                    |
| `atelier:working` | atelier | A builder holds it; a second one never starts    |
| `atelier:blocked` | Builder | It stopped; the pull request says what blocks it |
| `improvement`     | Critic  | A finding that does not block, kept for later    |

A builder that starts again on a blocked or stopped issue continues on the same
branch and pull request. Inside Working, core runs each issue through one state
machine: claiming, preparing a worktree, starting the builder, then following its
pull request from open to green to ready. Each step is one action atelier takes
and confirms, so a crash resumes where it stopped.

## What lives where

| atelier holds              | A project holds                                 |
| -------------------------- | ----------------------------------------------- |
| Core, plugins, CLI         | `.atelier/config.toml`: only what differs       |
| Builder and critic prompts | `AGENTS.md`: how to build, test and record it   |
| `gates.yml`, from `github` | `.github/workflows/atelier.yml`, which calls it |
| The pull request template  |                                                 |

Every setting has a default found from the project: the base is the forge's
default branch, the tracker and forge come from the `origin` remote, and the
languages from their roots. The config holds only what differs. Chauffeur's is one
line; another project might take only some issues, or turn checks off under a
path:

```toml
# .atelier/config.toml
base = "v2"

[filter]
labels = ["backend"] # atelier:build issues also labelled backend

[paths."vendor"]
checks = []         # nothing under vendor/ is checked
```

A filter narrows which `atelier:build` issues atelier takes, by `assignee`,
`author`, `labels` or the tracker's own query. Each tracker, listed as
`[[tracker]]`, has its own. Your own filters in `~/.config/atelier/projects.toml`,
one for each tracker, such as `assignee`, narrow them to your issues.

## Pull requests

Every pull request has the same sections, and a check fails when one is empty:

- **You can**: what a person can do once it merges.
- **Diagram**: Mermaid, checked to parse.
- **Recording**: a GIF that links to the video on R2.
- **Proofs**: each proof, what it covers, its result.
- **Gates**: the checks that ran.
- **Critic**: each finding, fixed or answered.

## Limits

Each loop ends. Each step has its `rounds`, five for the built-in `checks`: a
step's last failed round blocks the run unless a later step waits for that
failure, and atelier interrupts the builder, labels the issue `atelier:blocked`
and says why. A builder that only a person can unblock comments on the pull
request, labels the issue blocked and stops.

## Building atelier

atelier is built by atelier, one slice at a time. Its plan is
[issue #1](https://github.com/StevenJPx2/atelier/issues/1).
