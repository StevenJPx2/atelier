use std::collections::BTreeMap;

use super::{Plugins, Project, Record, State};
use crate::{Filter, Issue, IssueKey, Listing, Mark, PluginError, Seen};

/// What one poll saw of a project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observation {
    pub issues: BTreeMap<IssueKey, Observed>,
    /// Listing tags to keep once every issue is acted on.
    pub etags: BTreeMap<(String, Mark), String>,
}

/// One issue as the poll saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    pub issue: Issue,
    pub seen: Seen,
}

/// The marks atelier lists: issues asking for a builder, and issues
/// carrying the working mark, so one atelier never claimed is found.
const LISTED: [Mark; 2] = [Mark::Build, Mark::Working];

/// Asks the project's trackers what changed, reads every issue atelier is
/// working on, and asks the forge for each builder's pull request.
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

    for (key, record) in &state.issues {
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
        let Some(record) = state.issues.get(key) else {
            continue;
        };

        if record.phase.has_builder() {
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

const fn observed(issue: Issue) -> Observed {
    let seen = Seen {
        open: issue.open,
        marks: issue.marks,
        pull_request: None,
        stage: None,
    };

    Observed { issue, seen }
}

impl State {
    /// The issue's record, made when atelier first takes it.
    pub(crate) fn record(&mut self, issue: &Issue) -> &mut Record {
        self.issues
            .entry(issue.key.clone())
            .or_insert_with(|| Record::new(&issue.key, &issue.title))
    }
}
