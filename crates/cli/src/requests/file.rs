use atelier_core::{Command, IssueKey, Outcome, Request};
use serde::{Deserialize, Serialize};

/// One request for a pass of an item's pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct File {
    tracker: String,
    id: String,
    generation: u64,
    command: CommandFile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "snake_case", deny_unknown_fields)]
enum CommandFile {
    Finish {
        step: String,
        outcome: String,
        why: String,
        session: String,
        failed: u8,
    },
    Stop,
}

impl File {
    pub(super) fn new(key: &IssueKey, request: &Request) -> Self {
        let command = match &request.command {
            Command::Finish {
                step,
                outcome,
                session,
                failed,
            } => CommandFile::Finish {
                step: step.clone(),
                outcome: outcome.name.clone(),
                why: outcome.why.clone(),
                session: session.clone(),
                failed: *failed,
            },
            Command::Stop => CommandFile::Stop,
        };

        Self {
            tracker: key.tracker.clone(),
            id: key.id.clone(),
            generation: request.generation,
            command,
        }
    }

    pub(super) fn into_request(self) -> (IssueKey, Request) {
        let command = match self.command {
            CommandFile::Finish {
                step,
                outcome,
                why,
                session,
                failed,
            } => Command::Finish {
                step,
                outcome: Outcome { name: outcome, why },
                session,
                failed,
            },
            CommandFile::Stop => Command::Stop,
        };
        let key = IssueKey {
            tracker: self.tracker,
            id: self.id,
        };
        let request = Request {
            generation: self.generation,
            command,
        };

        (key, request)
    }
}
