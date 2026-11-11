//! `gh api` with its reply's status and headers, so a 304, a missing issue
//! and a rate limit each read as what they are.

use std::process::Command;
use std::time::Duration;

use atelier_core::{Failure, PluginError, Repo};

use crate::GitHub;

/// The longest one call to GitHub may take.
const DEADLINE: Duration = Duration::from_secs(30);

/// How long to wait when GitHub limits atelier without saying how long.
const DEFAULT_WAIT_SECS: u64 = 60;

/// A reply from the GitHub API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Reply {
    /// The header's value, by its name in any case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(found, _)| found.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The URL of the next page, from the `Link` header.
    pub fn next(&self) -> Option<&str> {
        self.header("link")?
            .split(',')
            .find(|link| link.contains("rel=\"next\""))?
            .split(['<', '>'])
            .nth(1)
    }

    /// The reply as a failure, unless it succeeded.
    ///
    /// # Errors
    ///
    /// A rate limit, a server error (transient) or any other error status
    /// (permanent).
    pub fn ok(self, what: &str) -> Result<Self, PluginError> {
        if (200..300).contains(&self.status) || self.status == 304 {
            return Ok(self);
        }

        let message = format!(
            "{what}: GitHub answered {} {}",
            self.status,
            self.body.trim()
        );
        let limited = self.status == 429
            || (self.status == 403
                && (self.header("retry-after").is_some()
                    || self.header("x-ratelimit-remaining") == Some("0")));

        let failure = if limited {
            let secs = self
                .header("retry-after")
                .and_then(|secs| secs.parse().ok());
            Failure::RateLimited {
                retry_after_secs: secs.unwrap_or(DEFAULT_WAIT_SECS),
            }
        } else if self.status >= 500 {
            Failure::Transient
        } else {
            Failure::Permanent
        };

        Err(PluginError { failure, message })
    }
}

impl GitHub {
    /// Runs `gh api --include` with `args`, and reads its reply whatever
    /// its status.
    ///
    /// # Errors
    ///
    /// `gh` cannot start, runs past its deadline, or prints no reply, such
    /// as when it cannot reach GitHub; all transient.
    pub(crate) fn api(&self, args: &[&str]) -> Result<Reply, PluginError> {
        let mut command = Command::new(&self.program);
        command
            .args(["api", "--include"])
            .args(args)
            .env("GH_PROMPT_DISABLED", "1");

        let output = atelier_command::run(command, DEADLINE)?;

        parse(&output.stdout).ok_or_else(|| {
            PluginError::transient(format!(
                "gh api {}: {}",
                args.join(" "),
                output.stderr.trim()
            ))
        })
    }
}

/// A reply printed by `gh api --include`: a status line, headers, a blank
/// line and the body.
pub fn parse(text: &str) -> Option<Reply> {
    let (head, body) = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .unwrap_or((text, ""));

    let mut lines = head.lines();
    let status = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;

    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
        .collect();

    Some(Reply {
        status,
        headers,
        body: body.to_owned(),
    })
}

/// A repository's owner and name.
///
/// # Errors
///
/// The repository is not `owner/name`.
pub fn owner_and_name(repo: &Repo) -> Result<(&str, &str), PluginError> {
    match repo.0.split_once('/') {
        Some((owner, name)) if !owner.is_empty() && !name.is_empty() && !name.contains('/') => {
            Ok((owner, name))
        }
        _ => Err(PluginError::permanent(format!(
            "{} is not owner/name",
            repo.0
        ))),
    }
}

/// Percent-encodes everything but unreserved characters, for a query string
/// or a path segment.
pub fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(status: u16, headers: &[(&str, &str)]) -> Reply {
        let headers = headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()));
        Reply {
            status,
            headers: headers.collect(),
            body: String::new(),
        }
    }

    #[test]
    fn github_s_limits_wait_its_server_errors_retry_and_other_errors_stop() {
        let cases = [
            (
                reply(429, &[]),
                Failure::RateLimited {
                    retry_after_secs: 60,
                },
            ),
            (
                reply(429, &[("Retry-After", "7")]),
                Failure::RateLimited {
                    retry_after_secs: 7,
                },
            ),
            (
                reply(403, &[("Retry-After", "30")]),
                Failure::RateLimited {
                    retry_after_secs: 30,
                },
            ),
            (
                reply(403, &[("X-Ratelimit-Remaining", "0")]),
                Failure::RateLimited {
                    retry_after_secs: 60,
                },
            ),
            (
                reply(403, &[("X-Ratelimit-Remaining", "4996")]),
                Failure::Permanent,
            ),
            (reply(404, &[("Retry-After", "30")]), Failure::Permanent),
            (reply(502, &[]), Failure::Transient),
            (reply(499, &[]), Failure::Permanent),
        ];

        for (reply, failure) in cases {
            let status = reply.status;

            assert_eq!(reply.ok("call").unwrap_err().failure, failure, "{status}");
        }

        for status in [200, 204, 299, 304] {
            assert!(reply(status, &[]).ok("call").is_ok(), "{status}");
        }
    }
}
