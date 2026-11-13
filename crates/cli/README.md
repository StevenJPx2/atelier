# The `atelier` command

The binary: it assembles the plugins, reads your projects and each project's
config, keeps the state, and runs the daemon the Homebrew service keeps
alive.

```sh
# in a project's checkout: its labels on GitHub
atelier init

# watch it: atelier's own clone, and ~/.config/atelier/projects.toml
atelier add StevenJPx2/chauffeur

# poll every watched project; what the service runs
brew services start atelier

# each project's settings, where each came from, and each issue
atelier status

# in a checkout: its gates, run the ones the changes touch, fix staged files
atelier gates
atelier check
atelier fix --staged
```

```text
StevenJPx2/chauffeur  /Users/me/.local/share/atelier/clones/StevenJPx2/chauffeur.git
  base        v2                       set
  ...
  pipeline    build                    found
    worktree  git.worktree
    builder   agent.session → opencode.session
    checks    forge.checks → github.checks
    ...
  github#6    Open { rounds: 1 }       issue-6-docs-fix-the-dead-commit-messages-link  ses_…  #8
  github#12   Building                 issue-12-fix-the-dead-link  ses_…  -
```

Each pipeline shows its steps, each with its kind and the plugin a role's
kind resolved to; a pipeline atelier cannot run shows instead of the
settings, with its name and why. Each issue shows its phase, branch, session and pull request (`-` when it
has none). `status` reads only `state.json`: the pull request's number is
the one the daemon's last poll saw, so it makes no GitHub call.

```mermaid
flowchart LR
    Main["main.rs<br/>supervisor"] -- "one thread each" --> P1["poll: chauffeur"]
    Main --> P2["poll: atelier"]
    P1 -- "save, wait for disk" --> Writer["writer<br/>state.json"]
    P2 -- "save, wait for disk" --> Writer
    P1 -- "observe, act" --> Plugins["github, opencode, git"]
```

| File               | Holds                                                                |
| ------------------ | -------------------------------------------------------------------- |
| `main.rs`          | The commands                                                         |
| `commands.rs`      | `init`, `add` and `status`                                           |
| `gates.rs`         | `gates`, `check` and `fix`, in a checkout                            |
| `hook.rs`          | The pre-commit hook builders' worktrees use                          |
| `clone.rs`         | atelier's own bare clone of each project, made or finished when used |
| `paths.rs`         | Where your projects, state, clones and worktrees live (XDG, or `~/`) |
| `projects.rs`      | `projects.toml`: your projects and your own filters                  |
| `config.rs`        | `.atelier/config.toml` from `origin`, what is found, what is set     |
| `build.toml`       | The built-in `build` pipeline, unless the config writes its own      |
| `command_step.rs`  | `atelier`, the plugin of atelier's own kind of step, `command`       |
| `plugins.rs`       | The plugins in the build, and each project's choice of them          |
| `state.rs`         | `state.json`, written whole and renamed into place                   |
| `log.rs`           | One line per event, with a UTC time                                  |
| `daemon/mod.rs`    | The supervisor: rereads `projects.toml` every 5 s                    |
| `daemon/lock.rs`   | The lock that lets one daemon run, holding its process id            |
| `daemon/poll.rs`   | One project's polls: interval, soon after acting, back-off           |
| `daemon/writer.rs` | The one thread that writes `state.json`                              |

- **Its own clones.** The daemon reads each project's bare clone under
  `~/.local/share/atelier/clones`, never your checkout, so it needs no
  access to folders macOS guards, such as `~/Documents`.
- **One owner per issue.** Each project polls on its own thread and owns its
  issues, so two polls never claim the same issue and a slow project never
  holds up another.
- **On disk before acting.** Every save waits for the writer to write
  `state.json` (at most 30 s), so a phase is saved before its actions run.
- **Config from `origin`.** `.atelier/config.toml` is read from `origin`'s
  default branch; a `base` there names the base, and the rest is read from
  `origin/<base>`. A branch or a pull request cannot change its limits.
- **Timing.** A project polls every `poll` seconds (30 by default, at least
  15), again 5 s after it acted, and backs off on failure: as long as a rate
  limit asks, otherwise 30 s doubling to 10 minutes.
- **Stopping.** The daemon runs until it is stopped; since every action is
  saved first, it can be stopped at any moment.
