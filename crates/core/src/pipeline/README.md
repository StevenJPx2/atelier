# Pipelines

A project's pipelines, as its config writes them: each a list of steps, each
step a kind a plugin offers, waiting on outcomes of steps before it.
`Pipeline::read` checks one when the config loads, against the `Kinds` the
project can use, and refuses it with its name and the reason.

| File       | Holds                                                           |
| ---------- | --------------------------------------------------------------- |
| `mod.rs`   | `Pipeline`, `PipelineError`, `MAX_STEPS`, `PASSED`, `FAILED`    |
| `spec.rs`  | `StepSpec`, `Needs` and `Need`: one checked step                |
| `kinds.rs` | `Kinds`: each namespace, a plugin's or a role's, and its plugin |
| `read.rs`  | `Pipeline::read`: reading each step, and the config check       |

```rust
let kinds = registry.kinds(None, "github", &["github"]);
let pipeline = Pipeline::read("security-audit", &steps, &kinds, &tree)?;

pipeline.step("audit").map(StepSpec::resolved);
// → Some("opencode.session"), from uses = "agent.session"
```
