use super::Reason;

/// A mark on an item that shows a person its run. Each tracker maps them
/// to its own labels, such as `atelier:working` on GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum Mark {
    Build,
    Working,
    Blocked,
}

/// What atelier does through its plugins after a move; a step by its
/// position in the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Mark(Mark),
    Unmark(Mark),
    /// Starts the step, or starts it again after a failed round.
    Start(u8),
    /// Comments a resident step's session and branch on the item, once its
    /// start made one.
    Announce(u8),
    /// Interrupts each resident step's session and tells it why.
    Interrupt,
    /// Comments each step's outcome on the item.
    Report,
    CloseIssue,
    RemoveWorktree,
    Comment(Reason),
}
