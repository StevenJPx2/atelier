# Fixtures

What `opencode api` printed, captured from OpenCode v2.2.7 on 2026-11-11.

| File                       | Request                                                                  |
| -------------------------- | ------------------------------------------------------------------------ |
| `session-created.json`     | `post /api/session` with a title, a directory and three permission rules |
| `interrupted-idle.json`    | `post /api/session/<id>/interrupt` on that idle session                  |
| `session-not-found.json`   | `post /api/session/ses_doesnotexist/interrupt`: stdout                   |
| `session-not-found.stderr` | The same call: stderr                                                    |

The session was deleted after the capture.

`prompt-accepted.json` is `post /api/session/<id>/prompt` with a text, sent
to a live builder's session on 2026-11-12. A prompt to a missing session
prints `session-not-found.json` and `session-not-found.stderr`, as the
interrupt does.

From a session titled `#12 Fix the dead link` in an empty temporary folder,
captured from OpenCode v2.2.7 on 2026-11-12 and deleted after:

| File                      | Request                                                  |
| ------------------------- | -------------------------------------------------------- |
| `session-list-empty.json` | `get /api/session?directory=<folder>` before the session |
| `session-list-one.json`   | The same, once the session was made                      |
| `messages-empty.json`     | `get /api/session/<id>/message` before any prompt        |
| `messages-prompted.json`  | The same, after one prompt and its answer                |
