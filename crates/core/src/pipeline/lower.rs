use rungs::{Outcome, Plan, Step};

use super::{FAILED, Need, StepSpec};

impl StepSpec {
    /// The outcome called `name` as rungs numbers it: its place in
    /// `outcomes`, or `Outcome::FAILED`.
    #[must_use]
    pub fn outcome(&self, name: &str) -> Option<Outcome> {
        if name == FAILED {
            return Some(Outcome::FAILED);
        }

        let index = self.outcomes.iter().position(|outcome| outcome == name)?;

        u8::try_from(index).ok().map(Outcome)
    }

    /// The name of `outcome`, as the config writes it.
    #[must_use]
    pub fn outcome_name(&self, outcome: Outcome) -> Option<&str> {
        if outcome == Outcome::FAILED {
            return Some(FAILED);
        }

        self.outcomes
            .get(usize::from(outcome.0))
            .map(String::as_str)
    }
}

/// The checked steps as a rungs plan: each step's outcomes by number, its
/// rounds as attempts, and its needs by position.
pub(super) fn lower(steps: &[StepSpec]) -> Result<Plan, String> {
    let mut lowered = Vec::new();

    for step in steps {
        let outcomes = u8::try_from(step.outcomes.len())
            .map_err(|_| format!("{}: too many outcomes", step.id))?;
        let all = waits(steps, &step.needs.all)?;
        let any = waits(steps, &step.needs.any)?;

        lowered.push(
            Step::new(outcomes)
                .attempts(step.rounds)
                .after(all)
                .after_any(any),
        );
    }

    Plan::new(lowered).map_err(|error| error.to_string())
}

fn waits(steps: &[StepSpec], needs: &[Need]) -> Result<Vec<rungs::Need>, String> {
    needs
        .iter()
        .map(|need| {
            let (at, named) = steps
                .iter()
                .enumerate()
                .find(|(_, step)| step.id == need.step)
                .ok_or_else(|| format!("no step {}", need.step))?;
            let outcomes = need
                .outcomes
                .iter()
                .map(|name| {
                    named
                        .outcome(name)
                        .ok_or_else(|| format!("{} has no outcome {name}", named.id))
                })
                .collect::<Result<Vec<_>, _>>()?;

            Ok(rungs::Need::outcome(at, outcomes))
        })
        .collect()
}
