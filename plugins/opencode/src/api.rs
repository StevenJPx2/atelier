//! `opencode api`: one request to the running server.

use std::process::Command;
use std::time::Duration;

use atelier_core::{Failure, PluginError};

use crate::OpenCode;

/// The longest one request may take. Prompting returns once the message is
/// accepted, so even it is quick.
const DEADLINE: Duration = Duration::from_secs(30);

impl OpenCode {
    /// Sends `method path` with an optional JSON body, and returns the
    /// server's JSON reply.
    ///
    /// # Errors
    ///
    /// `opencode` cannot start, runs past its deadline, cannot reach its
    /// server, or the server answers 5xx: transient. Any 4xx: permanent.
    pub(crate) fn api(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<String, PluginError> {
        match self.request(method, path, body)? {
            Answer::Reply(reply) => Ok(reply),
            Answer::Missing(error) => Err(error),
        }
    }

    /// The same, except that something the server no longer has, such as a
    /// deleted session, answers none.
    ///
    /// # Errors
    ///
    /// As `api`, for every other failure.
    pub(crate) fn api_if_there(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<Option<String>, PluginError> {
        match self.request(method, path, body)? {
            Answer::Reply(reply) => Ok(Some(reply)),
            Answer::Missing(_) => Ok(None),
        }
    }

    fn request(&self, method: &str, path: &str, body: Option<&str>) -> Result<Answer, PluginError> {
        let mut command = Command::new(&self.program);
        command.args(["api", method, path]);

        if let Some(body) = body {
            command.args(["--data", body]);
        }

        let output = atelier_command::run(command, DEADLINE)?;

        if output.success {
            return Ok(Answer::Reply(output.stdout));
        }

        let detail = serde_json::from_str::<serde_json::Value>(&output.stdout)
            .ok()
            .and_then(|reply| reply.get("message")?.as_str().map(str::to_owned))
            .unwrap_or_else(|| output.stderr.trim().to_owned());

        let failure = if output.stderr.contains("HTTP 4") {
            Failure::Permanent
        } else {
            Failure::Transient
        };

        let error = PluginError {
            failure,
            message: format!("opencode api {method} {path}: {detail}"),
        };

        if output.stderr.contains("HTTP 404") {
            Ok(Answer::Missing(error))
        } else {
            Err(error)
        }
    }
}

/// What the server answered: a reply, or that it has no such thing.
enum Answer {
    Reply(String),
    Missing(PluginError),
}
