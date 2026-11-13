//! What a poll sees of one issue: whether it is open, its marks, and the
//! pull request on its branch.

use crate::Mark;

/// The marks an issue carries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Marks {
    build: bool,
    working: bool,
    blocked: bool,
}

impl Marks {
    #[must_use]
    pub const fn has(self, mark: Mark) -> bool {
        match mark {
            Mark::Build => self.build,
            Mark::Working => self.working,
            Mark::Blocked => self.blocked,
        }
    }

    #[must_use]
    pub const fn with(mut self, mark: Mark) -> Self {
        match mark {
            Mark::Build => self.build = true,
            Mark::Working => self.working = true,
            Mark::Blocked => self.blocked = true,
        }

        self
    }

    #[must_use]
    pub const fn without(mut self, mark: Mark) -> Self {
        match mark {
            Mark::Build => self.build = false,
            Mark::Working => self.working = false,
            Mark::Blocked => self.blocked = false,
        }

        self
    }
}

impl FromIterator<Mark> for Marks {
    fn from_iter<I: IntoIterator<Item = Mark>>(marks: I) -> Self {
        marks.into_iter().fold(Self::default(), Self::with)
    }
}

/// The combined state of the checks on a pull request's head commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rollup {
    Pending,
    Passed,
    Failed,
}

/// A pull request on an issue's branch, as its forge reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequest {
    /// `failed_rounds` counts head commits that had a failed check.
    Open {
        number: u64,
        draft: bool,
        failed_rounds: u8,
        checks: Rollup,
    },
    Merged {
        number: u64,
    },
    Closed {
        number: u64,
    },
}

impl PullRequest {
    /// The pull request's number on its forge.
    #[must_use]
    pub const fn number(self) -> u64 {
        match self {
            Self::Open { number, .. } | Self::Merged { number } | Self::Closed { number } => number,
        }
    }
}

/// One issue as a poll saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    pub open: bool,
    pub marks: Marks,
    pub pull_request: Option<PullRequest>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_are_added_and_removed_one_at_a_time() {
        let marks: Marks = [Mark::Build, Mark::Blocked].into_iter().collect();

        assert!(marks.has(Mark::Build) && marks.has(Mark::Blocked));
        assert!(!marks.has(Mark::Working));
        assert!(marks.with(Mark::Working).has(Mark::Working));
        assert!(!marks.without(Mark::Build).has(Mark::Build));
        assert_eq!(PullRequest::Merged { number: 3 }.number(), 3);
        assert_eq!(PullRequest::Closed { number: 4 }.number(), 4);
    }
}
