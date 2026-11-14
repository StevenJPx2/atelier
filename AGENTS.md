# Working on atelier

On a developer's machine, use file tools, `git`, `gh`, and `cargo fmt`
only. Do not run Cargo build, check, Clippy, test, doc or metadata; nextest,
Kani, mutants; site or documentation builds; or the atelier binary. This
applies to child agents too. Do not run a local container as a workaround.

Every check runs on GitHub-hosted CI. The planner and all gates execute as a
non-root user inside a Linux container with a private PID namespace, no
capabilities, no host network, no Docker socket and no credentials, with
bounded memory, processes, CPU and time; a separate offline container
validates the Kani and mutation reports, and a missing or empty one fails.
[The CI runner](https://github.com/wrkhse/atelier/tree/main/.github/ci)
describes it. Never weaken a gate or exclude signalling code to obtain a pass.

Atelier's own pull requests run the workflow and `.github/ci` they contain,
so a change to either needs review for what it loosens; CI cannot catch a
pull request that disables its own containment.

Do not edit the user's atelier configuration, durable state, managed clones
or historical worktree paths as part of development. Use a separate branch
and worktree for each pull request, and preserve other branches' work.
