use rungs::Status;

use crate::{Event, FAILED, Outcome, Phase, Pipeline, Record};

/// A request for one pass of an item's pipeline. A later pass ignores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub generation: u64,
    pub command: Command,
}

/// What an agent step or a person requests of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Finish {
        step: String,
        outcome: Outcome,
        session: String,
        failed: u8,
    },
    Stop,
}

/// The first request that still applies to this run.
pub(super) fn requested(
    requests: &[Request],
    record: Option<&Record>,
    pipeline: Option<&Pipeline>,
) -> Option<(Event, Option<String>)> {
    let (record, pipeline) = record.zip(pipeline)?;
    let stop = requests
        .iter()
        .filter(|request| matches!(request.command, Command::Stop))
        .find_map(|request| request.event(record, pipeline));

    stop.or_else(|| {
        requests
            .iter()
            .find_map(|request| request.event(record, pipeline))
    })
}

impl Request {
    /// The event it means for this run, if it still applies. A finish
    /// names a running resident step in the same session and round.
    pub(super) fn event(
        &self,
        record: &Record,
        pipeline: &Pipeline,
    ) -> Option<(Event, Option<String>)> {
        if self.generation != record.generation || !record.phase.is_working() {
            return None;
        }

        let Command::Finish {
            step,
            outcome,
            session,
            failed,
        } = &self.command
        else {
            return Some((Event::Stop, None));
        };

        if record.phase != Phase::Running || !record.pending.is_empty() {
            return None;
        }

        let index = pipeline.steps().iter().position(|spec| &spec.id == step)?;
        let spec = pipeline.steps().get(index)?;
        let kept = record.steps.get(index)?;
        let current = kept.session.as_ref()?;

        if !spec.resident
            || &current.id != session
            || record.progress.status.get(index) != Some(&Status::Running)
            || record.progress.failed.get(index) != Some(failed)
        {
            return None;
        }

        let step = u8::try_from(index).ok()?;
        let event = if outcome.name == FAILED {
            Event::Failed { step, last: false }
        } else {
            Event::Settled {
                step,
                outcome: spec.outcome(&outcome.name)?,
            }
        };

        Some((event, Some(outcome.why.clone())))
    }
}
