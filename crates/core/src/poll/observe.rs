use std::collections::BTreeMap;

use super::{Plugins, Project, Record, State};
use crate::{Filter, Issue, IssueKey, Listing, Mark, Phase, PluginError, Seen};

/// What one poll saw of a project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observation {
    pub issues: BTreeMap<IssueKey, Watched>,
    /// Requests the project's worker read for this poll.
    pub requests: BTreeMap<IssueKey, Vec<super::Request>>,
    /// Listing tags to keep once every issue is acted on.
    pub etags: BTreeMap<(String, Mark), String>,
}

/// One issue as the poll saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watched {
    pub issue: Issue,
    pub seen: Seen,
}

/// The marks atelier lists: issues asking for a builder, and issues
/// carrying the working mark, so one atelier never claimed is found.
const LISTED: [Mark; 2] = [Mark::Build, Mark::Working];

/// Asks the project's trackers what changed, reads every issue atelier is
/// working on, and asks the forge for the pull request of each run whose
/// pipeline has a step of the forge's.
///
/// # Errors
///
/// The first plugin failure; the poll backs off and tries again.
pub fn observe(
    project: &Project,
    plugins: &Plugins<'_>,
    state: &State,
) -> Result<Observation, PluginError> {
    let mut observation = Observation::default();

    for source in &plugins.trackers {
        for mark in LISTED {
            list(project, source, mark, state, &mut observation)?;
        }
    }

    for (key, record) in &state.runs {
        if !record.phase.is_working() || observation.issues.contains_key(key) {
            continue;
        }

        let Some(source) = plugins.source(&key.tracker) else {
            continue;
        };

        let issue = source.tracker.issue(&project.repo, &key.id)?;
        observation.issues.insert(key.clone(), observed(issue));
    }

    for (key, observed) in &mut observation.issues {
        let Some(record) = state.runs.get(key) else {
            continue;
        };
        let forged = project.pipeline(&record.pipeline).is_some_and(|pipeline| {
            pipeline
                .steps()
                .iter()
                .any(|step| step.plugin == project.forge)
        });

        if record.phase == Phase::Running && forged {
            observed.seen.pull_request =
                plugins.forge.pull_request(&project.repo, &record.branch)?;
        }
    }

    Ok(observation)
}

fn list(
    project: &Project,
    source: &super::Source<'_>,
    mark: Mark,
    state: &State,
    observation: &mut Observation,
) -> Result<(), PluginError> {
    let key = (source.name.to_owned(), mark);
    let etag = state.etags.get(&key).map(String::as_str);
    let filter = if mark == Mark::Build {
        source.filter.clone()
    } else {
        Filter::default()
    };

    let Listing::Changed { issues, etag } =
        source.tracker.issues(&project.repo, mark, &filter, etag)?
    else {
        return Ok(());
    };

    for issue in issues {
        observation
            .issues
            .entry(issue.key.clone())
            .or_insert_with(|| observed(issue));
    }

    if let Some(etag) = etag {
        observation.etags.insert(key, etag);
    }

    Ok(())
}

const fn observed(issue: Issue) -> Watched {
    let seen = Seen {
        open: issue.open,
        marks: issue.marks,
        pull_request: None,
    };

    Watched { issue, seen }
}

impl State {
    /// The item's run, made when atelier first takes it.
    pub(crate) fn record(&mut self, issue: &Issue, pipeline: &str) -> &mut Record {
        self.runs
            .entry(issue.key.clone())
            .or_insert_with(|| Record::new(&issue.key, &issue.title, pipeline))
    }
}
