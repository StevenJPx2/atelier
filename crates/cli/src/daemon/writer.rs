//! The one thread that owns `state.json`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread;
use std::time::Duration;

use atelier_core::State;
use color_eyre::eyre::{Result, eyre};

use crate::state;

/// How many saves may wait for the writer; each poll thread waits for its
/// own, so a few is plenty.
const QUEUE: usize = 16;

/// How long a save waits for the writer before its poll gives up.
const WAIT: Duration = Duration::from_secs(30);

enum Message {
    Save {
        repo: String,
        state: State,
        done: SyncSender<Result<(), String>>,
    },
    Load {
        repo: String,
        reply: SyncSender<State>,
    },
}

/// A handle on the writer; cloned into each poll thread.
#[derive(Clone)]
pub struct Writer(SyncSender<Message>);

/// Loads `path` and starts its writer.
///
/// # Errors
///
/// The state file cannot be read.
pub fn start(path: PathBuf) -> Result<Writer> {
    let projects = state::load(&path)?;
    let (sender, receiver) = sync_channel(QUEUE);

    thread::Builder::new()
        .name("state writer".into())
        .spawn(move || serve(&path, projects, &receiver))?;

    Ok(Writer(sender))
}

fn serve(
    path: &std::path::Path,
    mut projects: BTreeMap<String, State>,
    receiver: &Receiver<Message>,
) {
    for message in receiver {
        match message {
            Message::Save { repo, state, done } => {
                projects.insert(repo, state);
                let written = state::write(path, &projects).map_err(|error| format!("{error:#}"));
                let _ = done.send(written);
            }
            Message::Load { repo, reply } => {
                let _ = reply.send(projects.get(&repo).cloned().unwrap_or_default());
            }
        }
    }
}

impl Writer {
    /// Writes the project's state, and returns once it is on disk.
    ///
    /// # Errors
    ///
    /// The writer is gone, too slow, or failed to write.
    pub fn save(&self, repo: &str, state: &State) -> Result<()> {
        let (done, wait) = sync_channel(1);
        let message = Message::Save {
            repo: repo.to_owned(),
            state: state.clone(),
            done,
        };

        self.0
            .send(message)
            .map_err(|_| eyre!("the state writer stopped"))?;

        wait.recv_timeout(WAIT)
            .map_err(|_| eyre!("the state writer did not answer"))?
            .map_err(|error| eyre!("writing the state: {error}"))
    }

    /// The project's state as last saved.
    ///
    /// # Errors
    ///
    /// The writer is gone or too slow.
    pub fn load(&self, repo: &str) -> Result<State> {
        let (reply, wait) = sync_channel(1);

        self.0
            .send(Message::Load {
                repo: repo.to_owned(),
                reply,
            })
            .map_err(|_| eyre!("the state writer stopped"))?;

        wait.recv_timeout(WAIT)
            .map_err(|_| eyre!("the state writer did not answer"))
    }
}
