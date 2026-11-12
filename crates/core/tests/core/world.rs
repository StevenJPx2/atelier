//! An in-memory tracker, forge, agent and worktrees, sharing one world, so
//! a poll runs end to end without I/O. `fail` makes the nth call that
//! changes the world fail.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test support: a fixture that breaks fails the test that uses it"
)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use atelier_core::{
    Agent, Brief, CiFile, Failure, Filter, Forge, Issue, IssueKey, Listing, Mark, Marks,
    Observation, Pipeline, PluginError, Plugins, Project, PullRequest, Repo, Report, Session,
    Source, State, Tracker, WorktreeRequest, Worktrees, act, observe,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    pub title: String,
    pub open: bool,
    pub marks: Marks,
    pub assignee: Option<String>,
    pub comments: Vec<String>,
}

#[derive(Default)]
pub struct World {
    pub tickets: RefCell<BTreeMap<String, Ticket>>,
    pub pulls: RefCell<BTreeMap<String, PullRequest>>,
    pub worktrees: RefCell<BTreeMap<PathBuf, String>>,
    pub sessions: RefCell<Vec<Session>>,
    pub interrupts: RefCell<Vec<String>>,
    /// Each message a session was sent, after its id.
    pub told: RefCell<Vec<(String, String)>>,
    pub briefs: RefCell<Vec<Brief>>,
    pub changes: RefCell<u32>,
    pub reads: RefCell<u32>,
    pub fail: RefCell<Option<(u32, Failure)>>,
    /// The project's filter for this tracker; the world honours its assignee.
    pub filter: RefCell<Filter>,
}

/// A project on base `v2` with `red_rounds = 3` and the given stages.
pub fn project(stages: &[(&str, u8)]) -> Project {
    Project {
        repo: Repo("o/r".into()),
        local: PathBuf::from("/local"),
        base: "v2".into(),
        pipeline: Pipeline::new(3, stages.iter().copied()).unwrap(),
    }
}

impl World {
    pub fn with_issue(id: &str, title: &str, marks: &[Mark]) -> Self {
        let world = Self::default();
        world.tickets.borrow_mut().insert(
            id.to_owned(),
            Ticket {
                title: title.to_owned(),
                open: true,
                marks: marks.iter().copied().collect(),
                assignee: None,
                comments: Vec::new(),
            },
        );
        world
    }

    pub fn ticket(&self, id: &str) -> Ticket {
        self.tickets.borrow()[id].clone()
    }

    pub fn edit(&self, id: &str, change: impl FnOnce(&mut Ticket)) {
        change(self.tickets.borrow_mut().get_mut(id).unwrap());
    }

    pub fn plugins(&self) -> Plugins<'_> {
        Plugins {
            trackers: vec![Source {
                name: "mem",
                tracker: self,
                filter: self.filter.borrow().clone(),
            }],
            forge: self,
            agent: self,
            worktrees: self,
        }
    }

    /// One poll, saving into `saved` as the daemon's worker would.
    pub fn poll(&self, project: &Project, state: &mut State) -> Report {
        let plugins = self.plugins();
        let observation: Observation = observe(project, &plugins, state).unwrap();
        let mut save = |_: &State| Ok::<(), ()>(());
        act(&observation, state, project, &plugins, &mut save).unwrap()
    }

    fn change(&self) -> Result<(), PluginError> {
        let mut changes = self.changes.borrow_mut();
        *changes += 1;

        match *self.fail.borrow() {
            Some((at, failure)) if at == *changes => Err(PluginError {
                failure,
                message: format!("call {at} fails"),
            }),
            _ => Ok(()),
        }
    }

    fn issue_of(id: &str, ticket: &Ticket) -> Issue {
        Issue {
            key: IssueKey {
                tracker: "mem".into(),
                id: id.to_owned(),
            },
            title: ticket.title.clone(),
            open: ticket.open,
            marks: ticket.marks,
        }
    }
}

impl Tracker for World {
    fn issues(
        &self,
        _: &Repo,
        mark: Mark,
        filter: &Filter,
        _: Option<&str>,
    ) -> Result<Listing, PluginError> {
        let issues = self
            .tickets
            .borrow()
            .iter()
            .filter(|(_, ticket)| ticket.open && ticket.marks.has(mark))
            .filter(|(_, ticket)| filter.assignee.is_none() || filter.assignee == ticket.assignee)
            .map(|(id, ticket)| Self::issue_of(id, ticket))
            .collect();

        Ok(Listing::Changed {
            issues,
            etag: Some(format!("{mark:?}")),
        })
    }

    fn issue(&self, _: &Repo, id: &str) -> Result<Issue, PluginError> {
        *self.reads.borrow_mut() += 1;
        Ok(Self::issue_of(id, &self.tickets.borrow()[id]))
    }

    fn mark(&self, _: &Repo, id: &str, mark: Mark) -> Result<(), PluginError> {
        self.change()?;
        self.edit(id, |ticket| ticket.marks = ticket.marks.with(mark));
        Ok(())
    }

    fn unmark(&self, _: &Repo, id: &str, mark: Mark) -> Result<(), PluginError> {
        self.change()?;
        self.edit(id, |ticket| ticket.marks = ticket.marks.without(mark));
        Ok(())
    }

    fn comment(&self, _: &Repo, id: &str, text: &str) -> Result<(), PluginError> {
        self.change()?;
        self.edit(id, |ticket| ticket.comments.push(text.to_owned()));
        Ok(())
    }

    fn close(&self, _: &Repo, id: &str) -> Result<(), PluginError> {
        self.change()?;
        self.edit(id, |ticket| ticket.open = false);
        Ok(())
    }

    fn reference(&self, issue: &Issue) -> String {
        format!("#{}", issue.key.id)
    }

    fn brief(&self, _: &Repo, issue: &Issue) -> String {
        format!("Read #{} with its comments.", issue.key.id)
    }
}

impl Forge for World {
    fn pull_request(&self, _: &Repo, branch: &str) -> Result<Option<PullRequest>, PluginError> {
        Ok(self.pulls.borrow().get(branch).copied())
    }

    fn brief(&self, _: &Repo, base: &str, branch: &str) -> String {
        format!("Open a pull request from {branch} into {base}.")
    }

    fn denied(&self) -> Vec<String> {
        vec!["merge *".into()]
    }

    fn ci(&self, _: &str, _: &str, _: &str) -> Vec<CiFile> {
        Vec::new()
    }
}

impl Agent for World {
    fn start(&self, _: &Path, brief: &Brief) -> Result<Session, PluginError> {
        self.change()?;
        let mut sessions = self.sessions.borrow_mut();
        let session = Session {
            id: format!("ses_{}", sessions.len()),
            title: brief.title.clone(),
        };
        sessions.push(session.clone());
        self.briefs.borrow_mut().push(brief.clone());
        Ok(session)
    }

    fn interrupt(&self, session: &Session) -> Result<(), PluginError> {
        self.change()?;
        self.interrupts.borrow_mut().push(session.id.clone());
        Ok(())
    }

    fn tell(&self, session: &Session, text: &str) -> Result<(), PluginError> {
        self.change()?;
        self.told
            .borrow_mut()
            .push((session.id.clone(), text.to_owned()));
        Ok(())
    }
}

impl Worktrees for World {
    fn create(&self, request: &WorktreeRequest<'_>) -> Result<PathBuf, PluginError> {
        self.change()?;
        let path = Path::new("/worktrees").join(request.name);
        self.worktrees
            .borrow_mut()
            .insert(path.clone(), request.branch.to_owned());
        Ok(path)
    }

    fn remove(&self, _: &Path, worktree: &Path) -> Result<(), PluginError> {
        self.change()?;
        self.worktrees.borrow_mut().remove(worktree);
        Ok(())
    }
}
