//! Starting, telling and interrupting a builder's session.

use std::path::Path;

use atelier_core::{Agent, Brief, PluginError, Session};
use serde::Deserialize;
use serde_json::json;

use crate::{OpenCode, rules};

#[derive(Deserialize)]
struct Created {
    data: Info,
}

#[derive(Deserialize)]
struct Info {
    id: String,
}

impl Agent for OpenCode {
    /// Makes the session in `worktree` with the brief's title and rules,
    /// then prompts it with the brief. A session whose prompt fails is
    /// deleted, so no idle builder is left behind.
    fn start(&self, worktree: &Path, brief: &Brief) -> Result<Session, PluginError> {
        let body = json!({
            "title": brief.title,
            "location": { "directory": worktree },
            "permissions": rules(brief),
        });

        let reply = self.api("post", "/api/session", Some(&body.to_string()))?;
        let created: Created = serde_json::from_str(&reply)
            .map_err(|error| PluginError::permanent(format!("opencode's new session: {error}")))?;

        let id = created.data.id;
        let prompt = json!({ "text": brief.text() }).to_string();

        if let Err(error) = self.api("post", &format!("/api/session/{id}/prompt"), Some(&prompt)) {
            let _ = self.api("delete", &format!("/api/session/{id}"), None);
            return Err(error);
        }

        Ok(Session {
            id,
            title: brief.title.clone(),
        })
    }

    /// Interrupting an idle session succeeds.
    fn interrupt(&self, session: &Session) -> Result<(), PluginError> {
        self.api(
            "post",
            &format!("/api/session/{}/interrupt", session.id),
            None,
        )?;
        Ok(())
    }

    /// Prompts the session with `text`; an idle one starts a turn.
    fn tell(&self, session: &Session, text: &str) -> Result<(), PluginError> {
        let prompt = json!({ "text": text }).to_string();

        self.api(
            "post",
            &format!("/api/session/{}/prompt", session.id),
            Some(&prompt),
        )?;
        Ok(())
    }
}
