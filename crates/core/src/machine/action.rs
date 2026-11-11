use super::Reason;

/// A mark on an issue that shows a person its phase. Each tracker maps them
/// to its own labels, such as `atelier:working` on GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum Mark {
    Build,
    Working,
    Blocked,
}

/// What atelier does through its plugins after a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Mark(Mark),
    Unmark(Mark),
    CreateWorktree,
    StartBuilder,
    CommentSession,
    StartStage(u8),
    Interrupt,
    CloseIssue,
    RemoveWorktree,
    Comment(Reason),
}
