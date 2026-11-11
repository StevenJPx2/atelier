# atelier

A personal software factory. You plan a feature as a GitHub issue and add the
`atelier:build` label. atelier starts a builder agent in its own worktree, which
opens a pull request and works until every check passes, a critic agent finds
nothing blocking, and a recording shows the feature working. Then the pull request
is yours to review.

```sh
atelier init                         # in a project: labels and .atelier/config.toml
atelier add StevenJPx2/chauffeur     # watch it
brew services start atelier          # poll every minute
gh issue edit 12 --add-label atelier:build
```

## How a feature moves

```mermaid
sequenceDiagram
    participant You
    participant Atelier as atelier poller
    participant Builder as Builder agent
    participant PR as Pull request and CI
    participant Critic as Critic agent
    You->>Atelier: label issue atelier:build
    Atelier->>Builder: worktree and session
    Builder->>PR: draft pull request
    PR-->>Builder: failed check, via sourcefed
    Builder->>PR: fix and push
    PR-->>Builder: every check passes
    Builder->>Critic: review this
    Critic->>PR: findings with evidence
    Builder->>PR: fix, record, mark ready
    PR-->>You: ready for review
```

The checks decide when the work is done. The agent does not judge it, and no
rulebook or skill has to be followed: a builder knows its issue, the project's
`AGENTS.md` and the contract in [`prompts/builder.md`](prompts/builder.md).

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
branch and pull request.

## What lives where

| atelier holds                                                   | A project holds                                     |
| --------------------------------------------------------------- | --------------------------------------------------- |
| The poller and CLI                                              | `.atelier/config.toml`: base branch, checks, limits |
| The builder and critic prompts                                  | `AGENTS.md`: how to build, test and record it       |
| Reusable workflows: Rust checks, mutants, Kani, TLA+, PR body   | A `check.yml` that calls them, pinned by commit     |
| The pull request template and labels, written by `atelier init` |                                                     |

```toml
# .atelier/config.toml
base = "v2"

[limits]
red_rounds = 5      # failed CI rounds before the builder stops
critic_rounds = 2
```

## Pull requests

Every pull request has the same sections, and a check fails when one is empty:

- **You can**: what a person can do once it merges.
- **Diagram**: Mermaid, checked to parse.
- **Recording**: a GIF that links to the video on R2.
- **Proofs**: each proof, what it covers, its result.
- **Gates**: the checks that ran.
- **Critic**: each finding, fixed or answered.

## Limits

Each loop ends. After `red_rounds` failed CI rounds or `critic_rounds` reviews,
the builder comments on the pull request with what blocks it, labels the issue
`atelier:blocked` and stops. The poller checks the same counts and stops a
builder that does not.

## Building atelier

atelier is built by atelier, one slice at a time. Its plan is
[issue #1](https://github.com/StevenJPx2/atelier/issues/1).
