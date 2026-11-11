/// Where one issue is in the pipeline.
///
/// `rounds` counts failed CI rounds on the pull request, and `index` is a
/// position in the project's stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum Phase {
    Untracked,
    Claiming,
    Preparing,
    Starting,
    Building,
    Open { rounds: u8 },
    Stage { index: u8, rounds: u8 },
    Green { rounds: u8 },
    Ready { rounds: u8 },
    Done,
    Blocked(Reason),
    Stopped,
}

/// Why an issue is blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum Reason {
    Start,
    RedRounds,
    Stage(u8),
    Closed,
    LostTrack,
}

/// Something a poll saw, or an action's confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum Event {
    Requested,
    Claimed,
    WorktreeReady,
    AgentStarted,
    StartFailed,
    PullRequestOpened,
    Pushed,
    RoundFailed,
    AllPassed,
    StagePassed,
    StageFailed { rounds: u8 },
    MarkedReady,
    Merged,
    ClosedUnmerged,
    Unmarked,
    IssueClosed,
    LostTrack,
}

impl Phase {
    /// Claiming to Ready: the issue carries the working mark.
    #[must_use]
    pub const fn is_working(self) -> bool {
        matches!(self, Self::Claiming | Self::Preparing | Self::Starting) || self.has_builder()
    }

    /// Building to Ready: a builder session may be running.
    #[must_use]
    pub const fn has_builder(self) -> bool {
        matches!(self, Self::Building) || self.has_pull_request()
    }

    /// Open to Ready: the builder's pull request exists.
    #[must_use]
    pub const fn has_pull_request(self) -> bool {
        matches!(
            self,
            Self::Open { .. } | Self::Stage { .. } | Self::Green { .. } | Self::Ready { .. }
        )
    }

    /// Failed CI rounds so far; zero before the pull request opens.
    #[must_use]
    pub const fn rounds(self) -> u8 {
        match self {
            Self::Open { rounds }
            | Self::Stage { rounds, .. }
            | Self::Green { rounds }
            | Self::Ready { rounds } => rounds,
            _ => 0,
        }
    }
}
