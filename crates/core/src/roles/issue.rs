use std::fmt;

use crate::Marks;

/// A repository on a tracker or forge, such as `StevenJPx2/chauffeur`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Repo(pub String);

/// An issue's key: the tracker it came from and its id there, so two
/// trackers never collide. Shown as `github#12`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IssueKey {
    pub tracker: String,
    pub id: String,
}

/// An issue as its tracker reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub key: IssueKey,
    pub title: String,
    pub open: bool,
    pub marks: Marks,
}

/// Which issues a tracker gives atelier, beyond carrying the mark. Every
/// field given must match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub assignee: Option<String>,
    pub author: Option<String>,
    pub labels: Vec<String>,
    pub query: Option<String>,
}

/// A tracker's answer to a listing: unchanged since the tag atelier holds,
/// or the issues with the tag to send next time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listing {
    Unchanged,
    Changed {
        issues: Vec<Issue>,
        etag: Option<String>,
    },
}

/// A builder's session in its agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub title: String,
}

impl fmt::Display for IssueKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.tracker, self.id)
    }
}
