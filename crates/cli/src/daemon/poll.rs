//! One project's polls, on their own thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use atelier_core::{Failure, PluginError, Report, State, act, observe};
use atelier_git::Git;
use color_eyre::eyre::Result;

use super::writer::Writer;
use crate::clone;
use crate::config;
use crate::log::log;
use crate::paths::Paths;
use crate::plugins::Build;
use crate::projects::Entry;

/// How soon a project is polled again after it acted, to follow up quickly.
const SOON: Duration = Duration::from_secs(5);

/// The longest back-off after repeated failures.
const MAX_BACKOFF: Duration = Duration::from_secs(600);

/// How often a waiting poll checks whether it should stop.
const TICK: Duration = Duration::from_millis(250);

/// Starts polling `entry` until `stop` is set.
///
/// # Errors
///
/// The thread cannot be started.
pub fn spawn(
    entry: Entry,
    paths: Paths,
    writer: Writer,
    stop: Arc<AtomicBool>,
) -> Result<JoinHandle<()>> {
    let name = format!("poll {}", entry.repo);

    Ok(thread::Builder::new()
        .name(name)
        .spawn(move || run(&entry, &paths, &writer, &stop))?)
}

fn run(entry: &Entry, paths: &Paths, writer: &Writer, stop: &AtomicBool) {
    let git = Git::new(paths.worktrees());
    let mut failures: u32 = 0;
    let mut state = match writer.load(&entry.repo) {
        Ok(state) => state,
        Err(error) => return log(&entry.repo, &format!("{error:#}")),
    };

    log(&entry.repo, "watching");

    while !stop.load(Ordering::Relaxed) {
        let wait = match once(entry, paths, &git, &mut state, writer) {
            Ok((report, poll)) => {
                failures = 0;
                tell(&entry.repo, &report);
                if report.steps.is_empty() { poll } else { SOON }
            }
            Err(error) => {
                failures = failures.saturating_add(1);
                log(&entry.repo, &format!("{error:#}"));
                backoff(&error, failures)
            }
        };

        sleep(wait, stop);
    }
}

fn once(
    entry: &Entry,
    paths: &Paths,
    git: &Git,
    state: &mut State,
    writer: &Writer,
) -> Result<(Report, Duration)> {
    let local = clone::ensure(paths, &entry.repo)?;
    let resolved = config::resolve(entry, &local)?;
    let instances = Build::new();
    let built = instances.plugins(&resolved)?;
    let plugins = built.plugins(git);

    let observation = observe(&resolved.project, &plugins, state)?;
    let mut save = |state: &State| writer.save(&entry.repo, state);
    let report = act(&observation, state, &resolved.project, &plugins, &mut save)?;

    Ok((report, resolved.poll))
}

fn tell(repo: &str, report: &Report) {
    for step in &report.steps {
        log(
            repo,
            &format!("{} {:?} → {:?}", step.key, step.from, step.to),
        );
    }

    for (key, error) in &report.failures {
        log(repo, &format!("{key}: {error}"));
    }
}

/// A rate limit waits as long as asked; other failures wait 30 seconds,
/// doubling each time, up to ten minutes.
fn backoff(error: &color_eyre::Report, failures: u32) -> Duration {
    if let Some(PluginError {
        failure: Failure::RateLimited { retry_after_secs },
        ..
    }) = error.downcast_ref()
    {
        return Duration::from_secs(*retry_after_secs).min(MAX_BACKOFF);
    }

    let doubling = 2_u32.saturating_pow(failures.saturating_sub(1).min(5));

    Duration::from_secs(30)
        .saturating_mul(doubling)
        .min(MAX_BACKOFF)
}

fn sleep(wait: Duration, stop: &AtomicBool) {
    let started = Instant::now();

    while !stop.load(Ordering::Relaxed) && started.elapsed() < wait {
        thread::sleep(TICK);
    }
}

#[cfg(test)]
mod tests {
    use color_eyre::eyre::eyre;

    use super::*;

    #[test]
    fn a_rate_limit_waits_as_asked_and_other_failures_double_up_to_ten_minutes() {
        let limited = |secs| {
            color_eyre::Report::new(PluginError {
                failure: Failure::RateLimited {
                    retry_after_secs: secs,
                },
                message: String::new(),
            })
        };
        let failed = eyre!("offline");
        let secs = |failures| backoff(&failed, failures).as_secs();

        assert_eq!(backoff(&limited(90), 1), Duration::from_secs(90));
        assert_eq!(backoff(&limited(9_000), 1), MAX_BACKOFF);
        assert_eq!(
            [1, 2, 3, 4, 5, 6, 9].map(secs),
            [30, 60, 120, 240, 480, 600, 600]
        );
    }

    /// Waits on another thread with a bound, so a wait that never ends
    /// fails the test instead of hanging it.
    fn timed(wait: Duration, stop: bool) -> Option<Duration> {
        let (done, finished) = std::sync::mpsc::channel();

        thread::spawn(move || {
            let started = Instant::now();
            sleep(wait, &AtomicBool::new(stop));
            let _ = done.send(started.elapsed());
        });

        finished.recv_timeout(Duration::from_secs(3)).ok()
    }

    #[test]
    fn a_wait_lasts_its_time_unless_told_to_stop() {
        let waited = timed(Duration::from_millis(600), false).expect("the wait ended");
        let cut = timed(Duration::from_secs(5), true).expect("the stopped wait ended");

        assert!(
            waited >= Duration::from_millis(600) && waited < Duration::from_secs(2),
            "{waited:?}"
        );
        assert!(cut < Duration::from_millis(100), "{cut:?}");
    }
}
