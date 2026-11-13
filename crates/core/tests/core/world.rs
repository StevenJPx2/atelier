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
    Agent, Brief, CiFile, Failure, Filter, Forge, Issue, IssueKey, Kind, Kinds, Listing, Mark,
    Marks, MemoryTree, Observation, Observed, Outcomes, Pipeline, PluginError, Plugins, Project,
    PullRequest, Repo, Report, RunContext, Session, Setting, Settings, SettingsError, Source,
    Started, State, Step, StepSpec, Tracker, Tree, WorktreeRequest, Worktrees, act, forge_kinds,
    observe, observe_builder, observe_forge_step, start_forge_step,
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
    /// Each step started, by its id, in order.
    pub started: RefCell<Vec<String>>,
    /// Each step observed, by its id, in order.
    pub observed: RefCell<Vec<String>>,
    /// What each `command` step reports, by its id; running until set.
    pub commands: RefCell<BTreeMap<String, Observed>>,
}

fn text(value: &str) -> Setting {
    Setting::Text(value.into())
}

/// A step's settings: its id, its kind, and the rest.
pub fn step(id: &str, uses: &str, rest: &[(&str, Setting)]) -> Settings {
    rest.iter().fold(
        Settings::default()
            .with("id", text(id))
            .with("uses", text(uses)),
        |step, (key, value)| step.with(key, value.clone()),
    )
}

pub fn list(values: &[&str]) -> Setting {
    Setting::List(values.iter().map(|value| text(value)).collect())
}

pub fn table(entries: &[(&str, &str)]) -> Setting {
    Setting::Table(
        entries
            .iter()
            .fold(Settings::default(), |table, (key, value)| {
                table.with(key, text(value))
            }),
    )
}

/// The built-in build, written out, with three rounds of checks.
pub fn build() -> Vec<Settings> {
    vec![
        step("worktree", "git.worktree", &[]),
        step(
            "builder",
            "agent.session",
            &[
                ("brief", text("builder")),
                ("outcomes", list(&["opened", "nothing"])),
            ],
        ),
        step(
            "checks",
            "forge.checks",
            &[
                ("after", table(&[("builder", "opened")])),
                ("rounds", Setting::Number(3)),
                ("restart", Setting::Flag(true)),
            ],
        ),
        step("ready", "forge.ready", &[]),
        step("merge", "forge.merge", &[]),
    ]
}

impl World {
    /// A project on base `v2` whose `build` pipeline is `steps`.
    pub fn project_with(&self, steps: &[Settings]) -> Project {
        let kinds = Kinds::default()
            .with("git", "mem", self)
            .with("agent", "mem", self)
            .with("forge", "mem", self)
            .with("atelier", "mem", self);
        let build = Pipeline::read("build", steps, &kinds, &MemoryTree::default()).unwrap();

        Project {
            repo: Repo("o/r".into()),
            local: PathBuf::from("/local"),
            base: "v2".into(),
            forge: "mem".into(),
            pipelines: vec![build],
            steps: PathBuf::from("/steps"),
        }
    }

    /// A project running the built-in build.
    pub fn project(&self) -> Project {
        self.project_with(&build())
    }

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
            steps: vec![("mem", self)],
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
    fn start(&self, _: &Path, brief: &Brief, _: Option<&Session>) -> Result<Session, PluginError> {
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
impl Step for World {
    fn kinds(&self) -> Vec<Kind> {
        let mut kinds = forge_kinds();
        kinds.extend([
            Kind {
                name: "worktree",
                outcomes: Outcomes::Own(&["passed"]),
                resident: false,
            },
            Kind {
                name: "session",
                outcomes: Outcomes::Named,
                resident: true,
            },
            Kind {
                name: "command",
                outcomes: Outcomes::Own(&["passed"]),
                resident: false,
            },
        ]);
        kinds
    }

    fn configure(
        &self,
        _: &str,
        settings: &Settings,
        _: &dyn Tree,
    ) -> Result<Settings, SettingsError> {
        Ok(settings.clone())
    }

    fn start(&self, run: &RunContext<'_>, step: &StepSpec) -> Result<Started, PluginError> {
        self.started.borrow_mut().push(step.id.clone());

        match step.kind.as_str() {
            "worktree" => {
                let request = WorktreeRequest {
                    repo: run.repo,
                    local: run.local,
                    name: run.name,
                    branch: run.branch,
                    base: run.base,
                };

                Ok(Started::Worktree(self.create(&request)?))
            }
            "session" => {
                let worktree = run
                    .worktree
                    .ok_or_else(|| PluginError::permanent("the session has no worktree"))?;

                Ok(Started::Session(Agent::start(
                    self,
                    worktree,
                    run.brief,
                    run.session,
                )?))
            }
            "command" => {
                self.change()?;
                Ok(Started::Nothing)
            }
            kind => start_forge_step(kind, run),
        }
    }

    fn observe(&self, run: &RunContext<'_>, step: &StepSpec) -> Result<Observed, PluginError> {
        self.observed.borrow_mut().push(step.id.clone());

        match step.kind.as_str() {
            "worktree" if run.worktree.is_some() => Ok(Observed::Settled(atelier_core::Outcome {
                name: "passed".into(),
                why: "made".into(),
            })),
            "worktree" => Ok(Observed::Running),
            "session" => Ok(observe_builder(run)),
            "command" => Ok(self
                .commands
                .borrow()
                .get(&step.id)
                .cloned()
                .unwrap_or(Observed::Running)),
            kind => observe_forge_step(kind, run),
        }
    }
}
