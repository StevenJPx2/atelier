# `state.json`

Each project's runs and listing tags, in `~/.local/state/atelier/state.json`,
written whole to a temporary file and renamed into place. A file an earlier
rc wrote is read too: each issue becomes a run of the built-in `build`
where it was, and the next save writes it as runs.

| File       | Holds                                                          |
| ---------- | -------------------------------------------------------------- |
| `mod.rs`   | `load` and `write`, and each run as the file keeps it          |
| `older.rs` | Each issue as `0.1.0-rc.8` and earlier wrote it, read as a run |

```json
{ "pipeline": "build", "tracker": "github", "id": "12",
  "phase": { "name": "running" },
  "steps": [
    { "id": "worktree", "status": { "name": "settled", "outcome": 0 }, "failed": 0 },
    { "id": "builder", "status": { "name": "running" }, "failed": 0,
      "session": { "id": "ses_…", "title": "#12 Fix the dead link" } }
  ], … }
```

| An earlier rc's phase | Its run of `build`                           |
| --------------------- | -------------------------------------------- |
| Preparing             | `worktree` running                           |
| Starting, Building    | `builder` running                            |
| Open, Stage           | `builder` settled `opened`, `checks` running |
| Green                 | `ready` running                              |
| Ready                 | `merge` running                              |
| Done                  | every step settled                           |
