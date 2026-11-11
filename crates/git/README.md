# Worktrees

Each issue's builder works in its own worktree, on its own branch, made
through the `git` command and the user's own sign-ins.

```mermaid
flowchart TD
    Exists{"folder exists?"} -->|"on the branch"| Reuse["reuse it"]
    Exists -->|"on another branch"| Refuse["refuse"]
    Exists -->|"no"| Fetch["fetch origin base"]
    Fetch --> Local{"local branch?"}
    Local -->|"yes"| Add["worktree add, the branch"]
    Local -->|"no"| Remote{"on origin?"}
    Remote -->|"yes"| Track["worktree add, tracking origin"]
    Remote -->|"no"| New["worktree add, new from origin/base"]
```

| File          | Holds                                                    |
| ------------- | -------------------------------------------------------- |
| `worktree.rs` | `Git`, core's `Worktrees`, and where each worktree lives |
| `command.rs`  | Running `git` through `atelier-command`, never prompting |

```rust
let git = Git::new("/Users/me/.local/share/atelier/worktrees");

let tree = git.create(&WorktreeRequest {
    repo: &Repo("StevenJPx2/chauffeur".into()),
    checkout: Path::new("/Users/me/Documents/Projects/chauffeur"),
    name: "issue-12",
    branch: "issue-12-fix-the-dead-link",
    base: "v2",
})?;
// → /Users/me/.local/share/atelier/worktrees/StevenJPx2/chauffeur/issue-12
```

- A local branch is never reset, so a builder started again continues its
  work.
- Removing forces out what the builder left and keeps the branch; a worktree
  already gone is pruned from git's list.
- Every command runs in its own process group with a 120-second deadline,
  never prompts for credentials, and keeps at most 1 MiB of each output. On
  the deadline the whole group is killed, so a child such as `ssh` cannot
  hold it open.
- A failure to reach `origin` is transient; any other failure is permanent.
- Each folder name is letters, digits, `-`, `_` or `.`, so a repository's
  name cannot lead out of the root.
