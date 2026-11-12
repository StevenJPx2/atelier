//! Starting, telling and interrupting a builder's session.

use std::fmt::Write;
use std::path::Path;

use atelier_core::{Agent, Brief, PluginError, Session};
use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::json;

use crate::{OpenCode, rules};

#[derive(Deserialize)]
struct Created {
    data: Info,
}

#[derive(Deserialize)]
struct Listed {
    data: Vec<Info>,
}

#[derive(Deserialize)]
struct Info {
    id: String,
    #[serde(default)]
    title: String,
}

#[derive(Deserialize)]
struct Messages {
    data: Vec<IgnoredAny>,
}

impl Agent for OpenCode {
    /// Continues the worktree's session with the brief's title when one
    /// exists and is not `replacing`: a start the daemon stopped in the
    /// middle of made it. It gets the brief only if it never did. Otherwise
    /// makes the session with the brief's title and rules, then prompts it
    /// with the brief; a session whose prompt fails is deleted, so no idle
    /// builder is left behind.
    fn start(
        &self,
        worktree: &Path,
        brief: &Brief,
        replacing: Option<&Session>,
    ) -> Result<Session, PluginError> {
        let replaced = replacing.map(|session| session.id.as_str());

        if let Some(id) = self.existing(worktree, &brief.title, replaced)? {
            if self.unbriefed(&id)? {
                self.prompt(&id, &brief.text())?;
            }

            return Ok(Session {
                id,
                title: brief.title.clone(),
            });
        }

        let body = json!({
            "title": brief.title,
            "location": { "directory": worktree },
            "permissions": rules(brief),
        });

        let reply = self.api("post", "/api/session", Some(&body.to_string()))?;
        let created: Created = serde_json::from_str(&reply)
            .map_err(|error| PluginError::permanent(format!("opencode's new session: {error}")))?;

        let id = created.data.id;

        if let Err(error) = self.prompt(&id, &brief.text()) {
            let _ = self.api("delete", &format!("/api/session/{id}"), None);
            return Err(error);
        }

        Ok(Session {
            id,
            title: brief.title.clone(),
        })
    }

    /// Interrupting an idle session, or one that is gone, succeeds.
    fn interrupt(&self, session: &Session) -> Result<(), PluginError> {
        self.api_if_there(
            "post",
            &format!("/api/session/{}/interrupt", session.id),
            None,
        )?;
        Ok(())
    }

    /// Prompts the session with `text`; an idle one starts a turn. Telling a
    /// session that is gone succeeds: no builder is left to hear it.
    fn tell(&self, session: &Session, text: &str) -> Result<(), PluginError> {
        let prompt = json!({ "text": text }).to_string();

        self.api_if_there(
            "post",
            &format!("/api/session/{}/prompt", session.id),
            Some(&prompt),
        )?;
        Ok(())
    }
}

impl OpenCode {
    /// The id of the worktree's session titled `title`, other than
    /// `replaced`.
    fn existing(
        &self,
        worktree: &Path,
        title: &str,
        replaced: Option<&str>,
    ) -> Result<Option<String>, PluginError> {
        let path = format!("/api/session?directory={}", encode(worktree));
        let reply = self.api("get", &path, None)?;
        let listed: Listed = serde_json::from_str(&reply)
            .map_err(|error| PluginError::permanent(format!("opencode's sessions: {error}")))?;

        Ok(listed
            .data
            .into_iter()
            .find(|session| session.title == title && Some(session.id.as_str()) != replaced)
            .map(|session| session.id))
    }

    /// Whether the session has no messages yet, so it never got its brief.
    fn unbriefed(&self, id: &str) -> Result<bool, PluginError> {
        let reply = self.api("get", &format!("/api/session/{id}/message"), None)?;
        let messages: Messages = serde_json::from_str(&reply).map_err(|error| {
            PluginError::permanent(format!("opencode's messages for {id}: {error}"))
        })?;

        Ok(messages.data.is_empty())
    }

    fn prompt(&self, id: &str, text: &str) -> Result<(), PluginError> {
        let prompt = json!({ "text": text }).to_string();

        self.api("post", &format!("/api/session/{id}/prompt"), Some(&prompt))?;
        Ok(())
    }
}

/// `path` for a URL's query: every byte but letters, digits, `-._~` and `/`
/// percent-encoded.
fn encode(path: &Path) -> String {
    let mut encoded = String::new();

    for byte in path.to_string_lossy().bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }

    encoded
}
