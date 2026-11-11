use std::fmt;

/// The most stages a project may run, so a stage's index fits in a `u8`
/// and the machine stays small enough to prove.
pub const MAX_STAGES: usize = 8;

/// The limits an issue runs under, from the project's config: failed CI
/// rounds before it blocks, and each stage's own round limit, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pipeline {
    red_rounds: u8,
    limits: [u8; MAX_STAGES],
    count: u8,
    names: Vec<String>,
}

/// A pipeline the config describes but the machine cannot run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineError {
    NoRedRounds,
    NoStageRounds { stage: String },
    TooManyStages { count: usize },
}

impl Pipeline {
    /// # Errors
    ///
    /// Refuses a limit of zero and more than eight stages.
    pub fn new<'a>(
        red_rounds: u8,
        stages: impl IntoIterator<Item = (&'a str, u8)>,
    ) -> Result<Self, PipelineError> {
        if red_rounds == 0 {
            return Err(PipelineError::NoRedRounds);
        }

        let stages: Vec<(&str, u8)> = stages.into_iter().collect();

        if let Some((name, _)) = stages.iter().find(|(_, rounds)| *rounds == 0) {
            return Err(PipelineError::NoStageRounds {
                stage: (*name).to_owned(),
            });
        }

        let (Ok(count), true) = (u8::try_from(stages.len()), stages.len() <= MAX_STAGES) else {
            return Err(PipelineError::TooManyStages {
                count: stages.len(),
            });
        };

        let mut limits = [0; MAX_STAGES];

        for (limit, (_, rounds)) in limits.iter_mut().zip(&stages) {
            *limit = *rounds;
        }

        let names = stages.iter().map(|(name, _)| (*name).to_owned()).collect();

        Ok(Self {
            red_rounds,
            limits,
            count,
            names,
        })
    }

    /// A pipeline of unnamed stages, for proofs that range over limits.
    #[cfg(kani)]
    pub(crate) const fn unnamed(red_rounds: u8, limits: [u8; MAX_STAGES], count: u8) -> Self {
        Self {
            red_rounds,
            limits,
            count,
            names: Vec::new(),
        }
    }

    #[must_use]
    pub const fn red_rounds(&self) -> u8 {
        self.red_rounds
    }

    /// How many stages the project runs.
    #[must_use]
    pub const fn stages(&self) -> u8 {
        self.count
    }

    /// The stage's name, as the project's config gives it.
    #[must_use]
    pub fn stage_name(&self, index: u8) -> Option<&str> {
        self.names.get(usize::from(index)).map(String::as_str)
    }

    /// The stage's round limit; none past the last stage.
    #[must_use]
    pub fn limit(&self, index: u8) -> Option<u8> {
        if index < self.count {
            self.limits.get(usize::from(index)).copied()
        } else {
            None
        }
    }
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRedRounds => write!(f, "red_rounds must be at least 1"),
            Self::NoStageRounds { stage } => {
                write!(f, "{stage}_rounds must be at least 1")
            }
            Self::TooManyStages { count } => {
                write!(f, "{count} stages; atelier runs at most {MAX_STAGES}")
            }
        }
    }
}

impl std::error::Error for PipelineError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_keep_their_order_names_and_limits() {
        let pipeline = Pipeline::new(5, [("critic", 2), ("recording", 1)]).unwrap();

        assert_eq!(pipeline.stages(), 2);
        assert_eq!(pipeline.stage_name(1), Some("recording"));
        assert_eq!(pipeline.limit(0), Some(2));
        assert_eq!(pipeline.limit(1), Some(1));
        assert_eq!(pipeline.limit(2), None);
    }

    #[test]
    fn a_pipeline_the_machine_cannot_run_is_refused() {
        let nine = vec![("stage", 1); 9];

        let refused = [
            (Pipeline::new(0, []), "red_rounds must be at least 1"),
            (
                Pipeline::new(5, [("critic", 0)]),
                "critic_rounds must be at least 1",
            ),
            (Pipeline::new(5, nine), "9 stages; atelier runs at most 8"),
        ];

        for (pipeline, message) in refused {
            assert_eq!(pipeline.unwrap_err().to_string(), message);
        }
    }
}
