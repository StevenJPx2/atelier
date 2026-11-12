# Fixtures

Replies `gh api --include` printed, captured from GitHub on 2026-11-11 against
this repository, except where a row says otherwise.

| File                        | Request                                                                                            |
| --------------------------- | -------------------------------------------------------------------------------------------------- |
| `issues-with-pulls.txt`     | `repos/StevenJPx2/atelier/issues?state=all&per_page=6`: five pull requests and #7, and a next page |
| `issues-page-2.txt`         | The next page of `issues?labels=plan&state=open&per_page=4`: #3 to #1, the last page               |
| `issues-304.txt`            | `issues-with-pulls.txt` again with its `If-None-Match`                                             |
| `issue-6.txt`               | `repos/StevenJPx2/atelier/issues/6`                                                                |
| `issue-404.txt`             | `repos/StevenJPx2/atelier/issues/999999`                                                           |
| `pull-merged.txt`           | The GraphQL query for branch `feat/slice-0-setup`: #8, merged, a failed commit then a passing one  |
| `pull-open.txt`             | The query for `feat/git-worktrees`: #12, open, its one commit passing                              |
| `pull-none.txt`             | The query for a branch with no pull request                                                        |
| `pull-open-failed-once.txt` | **Derived** from `pull-merged.txt`, with #8 open and a draft                                       |
| `forbidden.txt`             | `POST repos/cli/cli/issues/1/labels` without the rights: a 403 that is no rate limit               |
| `rate-limited.txt`          | **Constructed** from GitHub's documented rate-limit reply                                          |

`git-protocol-https.txt` and `git-protocol-ssh.txt` are what
`gh config get git_protocol -h github.com` printed (gh 2.103.3, 2026-11-12)
with a fresh configuration, then after `gh config set git_protocol ssh`.
