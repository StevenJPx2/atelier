use rungs::{Outcome, Progress};

/// Where one run is: claiming its item, running its steps, or out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum Phase {
    Untracked,
    Claiming,
    Running,
    Done,
    Blocked(Reason),
    Stopped,
}

/// Why a run is blocked; a step by its position in the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum Reason {
    /// Its item could not be claimed.
    Claim,
    /// A step could not start.
    Start(u8),
    /// A step's final failure, which no step waits for.
    Failed(u8),
    /// The pipeline changed while it ran.
    Changed,
    /// The item carries the working mark, and atelier holds no run of it.
    LostTrack,
}

/// Something a poll saw, or an action's confirmation; a step by its
/// position in the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum Event {
    Requested,
    Claimed,
    ClaimFailed,
    Settled {
        step: u8,
        outcome: Outcome,
    },
    /// A failed round; `last` when no round can go better.
    Failed {
        step: u8,
        last: bool,
    },
    StartFailed {
        step: u8,
    },
    /// A settled step runs again, as checks do after a push.
    Pushed {
        step: u8,
    },
    Changed,
    Stop,
    Unmarked,
    IssueClosed,
    LostTrack,
}

/// One run's place: its phase, and each step's status and failed rounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub struct Run {
    pub phase: Phase,
    pub progress: Progress,
}

impl Phase {
    /// Claiming or running: the item carries the working mark.
    #[must_use]
    pub const fn is_working(self) -> bool {
        matches!(self, Self::Claiming | Self::Running)
    }
}

impl Run {
    /// A run not started yet.
    pub const UNTRACKED: Self = Self {
        phase: Phase::Untracked,
        progress: Progress {
            status: [rungs::Status::Pending; rungs::MAX_STEPS],
            failed: [0; rungs::MAX_STEPS],
        },
    };
}
