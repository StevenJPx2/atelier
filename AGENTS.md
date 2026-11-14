# Working on atelier

On this Mac, use file tools, `git`, `gh`, and `cargo fmt` only. Do not run
Cargo build, check, Clippy, test, doc or metadata; nextest, Kani, mutants;
site or documentation builds; or the atelier binary. This applies to child
agents too. Do not run a local container as a workaround.

Every check runs on GitHub-hosted CI. The planner and all gates execute as a
non-root user inside a resource-bounded Linux container with a private PID
namespace, no host network, no Docker socket, and no credentials. Missing or
empty mutation and Kani reports fail verification. Never weaken a gate or
exclude signalling code to obtain a pass.

Do not start or re-enable the installed atelier daemon. It remains unloaded
and disabled until the owner approves installing slice 3's release candidate
and starting it. Do not edit the user's atelier configuration, durable state,
managed clones or historical worktree paths as part of development.

Preserve other branches' dirty work and diagnostics. Use a separate branch and
worktree for each pull request. Do not merge, tag, release or deploy; the
supervisor owns merge approval. Slice 4 waits for slice 3's owner review.
