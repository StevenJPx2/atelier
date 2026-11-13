# `atelier.command`

atelier's own kind of step: a shell command run in the run's worktree, with
a time limit. `atelier` is the plugin that offers it, under its own name.

| File     | Holds                                                    |
| -------- | -------------------------------------------------------- |
| `mod.rs` | `Atelier`, the plugin, and checking a command's settings |
| `run.rs` | Starting a command in the background, and how it ended   |

```toml
[[pipeline.security-audit.step]]
id = "deps"
uses = "atelier.command"
run = "cargo deny check advisories"
timeout = 300
```

```rust
run::start(&folder, &worktree, "main", &step.settings)?;

run::observe(&folder, &step.settings)?;
// → None while it runs, then Some(Outcome { name: "failed", why: "it exited 3:\n…" })
```

- **In the background.** A start returns at once; a later poll reads how it
  ended from the step's folder: `started`, `group`, `output` and `exit`.
- **Its own process group,** not the daemon's, so a daemon restarted while it
  runs leaves it running, and its limit kills it with every process it
  started.
- **Outcomes.** `passed` on exit 0; `failed` on any other exit, past its
  limit, or when it stopped without an exit status, each with why and the
  last 20 lines it printed.
- **`ATELIER_BASE`** is `origin/<base>`, as for the gates.
