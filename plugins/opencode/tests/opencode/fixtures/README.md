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
