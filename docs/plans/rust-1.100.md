# Rust 1.100

**Status: In progress.** The workspace's minimum is Rust 1.100 and development
and CI pin stable `1.100.0`. Kani's compatibility with the raised minimum is
shown by this change's contained CI run; the simplifications below follow in
their own pull request.

Every member inherits `rust-version = "1.100"` from the workspace, and the
`msrv` gate checks it:

```toml
# .atelier/config.toml
[commands.msrv]
setup = ["rustup toolchain install 1.100.0 --profile minimal"]
run = "cargo +1.100.0 check --workspace --all-targets --all-features --locked"
```

No dependency, runtime behavior, gate or release version changes with the
toolchain. Edition 2024 stays.

## Verification

Every gate runs in CI's container, as [AGENTS.md](../../AGENTS.md) and
[slice 3](slices/slice-3.md#verification) describe; on a developer's Mac only
`cargo fmt` runs.

| Check                                     | Proves                                                             |
| ----------------------------------------- | ------------------------------------------------------------------ |
| `clippy`, `nextest`, `doctest`, `rustdoc` | The workspace builds and passes on 1.100.0                         |
| `msrv`                                    | The declared minimum holds                                         |
| `kani`                                    | Kani 0.68.0 still proves both core harnesses with the new manifest |
| `mutants`                                 | No mutant is missed on the changed lines                           |
| Package, Release                          | Native builds and smoke runs on macOS arm64 and Linux              |

Kani 0.68.0 compiles with its own `nightly-2026-08-21` (rustc 1.100.0-nightly,
commit `8925ea358`). Cargo ignores the nightly suffix when it compares
`rust-version`, so the manifest check passes; an API stabilized late in 1.100
may still be missing there. An API from 1.97 to 1.100 enters code Kani compiles
(`crates/core` and `rungs`) only when its stabilization is in that commit or a
contained Kani run passes with it.

Cargo 1.100 moves intermediate build artifacts. Atelier reads only final ones:
`target/<triple>/release/atelier` in releases, `target/release/atelier` in
packaging, and `CARGO_BIN_EXE_atelier` in the CLI tests. nextest 0.9.151 is past
the release that supports the new layout.

## Simplifications it allows

One follow-up pull request applies these, each with its regression check:

| Where                                 | Today                                   | With                                    | Since |
| ------------------------------------- | --------------------------------------- | --------------------------------------- | ----- |
| `crates/cli/src/config.rs:420`        | `strip_suffix(".git").unwrap_or(rest)`  | `trim_suffix(".git")`                   | 1.100 |
| `crates/core/src/poll/act.rs:632,678` | `map_or_else(String::new, …)`           | `map_or_default(…)`                     | 1.98  |
| `crates/command/src/lib.rs:82-83`     | `map(collect).unwrap_or_default()`      | `map_or_default(collect)`               | 1.98  |
| `crates/cli/src/plugins.rs:89-92`     | `map(…).unwrap_or_default()`            | `map_or_default(…)`                     | 1.98  |
| `crates/command/src/lib.rs:102`       | `from_utf8_lossy(&tail.0).into_owned()` | `String::from_utf8_lossy_owned(tail.0)` | 1.99  |

```rust
let rest = rest.trim_suffix(".git");

let stdout = stdout.map_or_default(collect);
```

`trim_suffix` removes one `.git`; the required `strip_prefix` and the repeated
`trim_end_matches('/')` stay, since they decide which URLs are accepted. The
`act.rs` sites are compiled by Kani, so they wait for its run.

Cargo's `unused_dependencies` lint waits until Kani is shown to accept the
manifest table; cargo-shear and cargo-deny keep their gates either way.

```toml
[workspace.lints.cargo]
unused_dependencies = "deny"
```

`DropGuard` does not replace process cleanup, a durable step folder or the
queue's reservation release: each stays explicit and returns its error, and no
`Drop` runs after a `SIGKILL`. No unsafe code, allocator, dependency cooldown or
target change comes with the toolchain.

## Sources

- [Rust 1.100 announcement](https://blog.rust-lang.org/2026/11/12/Rust-1.100.0/)
  and [release notes](https://doc.rust-lang.org/stable/releases.html), 1.97 to
  1.100.
- [Cargo's `rust-version`](https://doc.rust-lang.org/stable/cargo/reference/rust-version.html)
  and [Cargo lints](https://doc.rust-lang.org/stable/cargo/reference/lints.html).
- [Kani 0.68.0's toolchain](https://github.com/model-checking/kani/blob/kani-0.68.0/rust-toolchain.toml).
