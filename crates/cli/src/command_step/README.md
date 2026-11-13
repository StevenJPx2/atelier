# `atelier.command`

atelier's own kind of step: a shell command run in the run's worktree, with
a time limit. `atelier` is the plugin that offers it, under its own name.

| File     | Holds                                                    |
| -------- | -------------------------------------------------------- |
| `mod.rs` | `Atelier`, the plugin, and checking a command's settings |

```toml
[[pipeline.security-audit.step]]
id = "deps"
uses = "atelier.command"
run = "cargo deny check advisories"
timeout = 300
```
