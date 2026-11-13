# Roles

Each role is a trait a plugin implements, with the values that cross it. A
plugin's `register` adds the roles it fills to the `Registry`.

| File        | Holds                                                                 |
| ----------- | --------------------------------------------------------------------- |
| `traits.rs` | `Tracker`, `Forge`, `Agent` and `Worktrees`                           |
| `step.rs`   | `Step`, a plugin's kinds of step, each a `Kind` with its `Outcomes`   |
| `issue.rs`  | `Repo`, `IssueKey`, `Issue`, `Filter`, `Listing` and `Session`        |
| `error.rs`  | `PluginError` and its `Failure`: transient, rate-limited or permanent |

```rust
impl Plugin for GitHub {
    fn name(&self) -> &'static str {
        "github"
    }

    fn register<'a>(&'a self, registry: &mut Registry<'a>) {
        registry.tracker(self);
        registry.forge(self);
    }
}

let registry = Registry::new(&[&GitHub, &OpenCode]);

let tracker = registry.find_tracker("jira");
// → Err: this atelier has no tracker plugin "jira"; it has github
```
