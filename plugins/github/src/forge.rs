//! GitHub pull requests as atelier's forge.

use atelier_core::{CiFile, Forge, PluginError, PullRequest, Repo, Rollup};

use crate::ci;
use serde::Deserialize;

use crate::GitHub;
use crate::api::{api, owner_and_name};

/// The newest pull request on a branch, with each of its last 100 commits'
/// combined check state.
const QUERY: &str = "query($owner: String!, $name: String!, $branch: String!) {
  repository(owner: $owner, name: $name) {
    pullRequests(headRefName: $branch, first: 1,
                 orderBy: {field: CREATED_AT, direction: DESC},
                 states: [OPEN, MERGED, CLOSED]) {
      nodes { number state isDraft
              commits(last: 100) { nodes { commit { statusCheckRollup { state } } } } }
    }
  }
}";

#[derive(Deserialize)]
struct Reply {
    data: Option<Data>,
}

#[derive(Deserialize)]
struct Data {
    repository: Option<Repository>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repository {
    pull_requests: Nodes<Pull>,
}

#[derive(Deserialize)]
struct Nodes<T> {
    nodes: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pull {
    number: u64,
    state: String,
    is_draft: bool,
    commits: Nodes<CommitNode>,
}

#[derive(Deserialize)]
struct CommitNode {
    commit: Commit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Commit {
    status_check_rollup: Option<RollupReply>,
}

#[derive(Deserialize)]
struct RollupReply {
    state: String,
}

fn failed(rollup: Option<&RollupReply>) -> bool {
    rollup.is_some_and(|rollup| matches!(rollup.state.as_str(), "FAILURE" | "ERROR"))
}

impl Pull {
    /// A failed round is a commit whose checks failed; the head's checks are
    /// pending until they report, as a head with no checks is.
    fn pull_request(self) -> PullRequest {
        let number = self.number;

        match self.state.as_str() {
            "MERGED" => PullRequest::Merged { number },
            "CLOSED" => PullRequest::Closed { number },
            _ => {
                let rollups: Vec<Option<&RollupReply>> = self
                    .commits
                    .nodes
                    .iter()
                    .map(|node| node.commit.status_check_rollup.as_ref())
                    .collect();

                let failed_rounds = rollups.iter().filter(|rollup| failed(**rollup)).count();
                let head = rollups.last().copied().flatten();

                let checks = match head.map(|rollup| rollup.state.as_str()) {
                    Some("SUCCESS") => Rollup::Passed,
                    Some("FAILURE" | "ERROR") => Rollup::Failed,
                    _ => Rollup::Pending,
                };

                PullRequest::Open {
                    number,
                    draft: self.is_draft,
                    failed_rounds: u8::try_from(failed_rounds).unwrap_or(u8::MAX),
                    checks,
                }
            }
        }
    }
}

impl Forge for GitHub {
    fn pull_request(&self, repo: &Repo, branch: &str) -> Result<Option<PullRequest>, PluginError> {
        let (owner, name) = owner_and_name(repo)?;
        let what = format!("finding the pull request on {branch}");
        let fields = [
            format!("owner={owner}"),
            format!("name={name}"),
            format!("branch={branch}"),
        ];
        let query = format!("query={QUERY}");
        let mut args = vec!["graphql", "--raw-field", query.as_str()];

        for field in &fields {
            args.extend(["--raw-field", field.as_str()]);
        }

        let reply = api(&self.program, &args)?.ok(&what)?;
        let parsed: Reply = serde_json::from_str(&reply.body)
            .map_err(|error| PluginError::permanent(format!("{what}: GitHub's reply: {error}")))?;

        let repository = parsed
            .data
            .and_then(|data| data.repository)
            .ok_or_else(|| PluginError::permanent(format!("{what}: {} not found", repo.0)))?;

        Ok(repository
            .pull_requests
            .nodes
            .into_iter()
            .next()
            .map(Pull::pull_request))
    }

    fn brief(&self, repo: &Repo, base: &str, branch: &str) -> String {
        let repo = &repo.0;

        format!(
            "Open the pull request as a draft: \
             `gh pr create --draft --repo {repo} --base {base} --head {branch}`. \
             Then watch it with a sourcefed monitor (`sourceType: \"github\"`, \
             repo {repo} and the pull request's number), so failed checks and \
             reviews reach you as messages. When atelier tells you every check passed, \
             mark it ready: `gh pr ready --repo {repo}` with its number."
        )
    }

    fn denied(&self) -> Vec<String> {
        vec!["gh pr merge *".into(), "gh release *".into()]
    }

    fn ci(&self, release: &str, commit: &str, base: &str) -> Vec<CiFile> {
        vec![ci::workflow(release, commit, base)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull(state: &str, rollups: &[Option<&str>]) -> PullRequest {
        let commits = rollups
            .iter()
            .map(|rollup| {
                let rollup = rollup.map_or_else(
                    || "null".to_owned(),
                    |state| format!(r#"{{"state": "{state}"}}"#),
                );
                format!(r#"{{"commit": {{"statusCheckRollup": {rollup}}}}}"#)
            })
            .collect::<Vec<_>>()
            .join(",");
        let json = format!(
            r#"{{"number": 3, "state": "{state}", "isDraft": false, "commits": {{"nodes": [{commits}]}}}}"#
        );

        serde_json::from_str::<Pull>(&json).unwrap().pull_request()
    }

    const fn open(failed_rounds: u8, checks: Rollup) -> PullRequest {
        PullRequest::Open {
            number: 3,
            draft: false,
            failed_rounds,
            checks,
        }
    }

    #[test]
    fn each_state_and_head_reads_as_core_expects() {
        let cases = [
            (
                pull("CLOSED", &[Some("FAILURE")]),
                PullRequest::Closed { number: 3 },
            ),
            (
                pull("OPEN", &[Some("SUCCESS"), Some("FAILURE")]),
                open(1, Rollup::Failed),
            ),
            (
                pull("OPEN", &[Some("ERROR"), Some("ERROR")]),
                open(2, Rollup::Failed),
            ),
            (
                pull("OPEN", &[Some("FAILURE"), None]),
                open(1, Rollup::Pending),
            ),
            (pull("OPEN", &[Some("EXPECTED")]), open(0, Rollup::Pending)),
            (pull("OPEN", &[]), open(0, Rollup::Pending)),
        ];

        for (found, expected) in cases {
            assert_eq!(found, expected);
        }
    }
}
