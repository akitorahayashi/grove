//! Shared phase skeleton for the repository use cases.
//!
//! Each use case supplies its own phase marker, per-repository action, and
//! change predicate; this module owns the event envelope, timing, bounded
//! parallel or ordered sequential execution, and summary production common to
//! every phase.

use std::path::Path;
use std::time::Instant;

use crate::AppError;
use crate::repositories::RepositoryDefinition;

use super::events::{Event, EventSink, Summary};
use super::workers;

/// A unit of parallel repository work: which repository it concerns and which
/// Git resource (common directory) serializes it against sibling worktrees.
pub(crate) trait Task {
    fn repository(&self) -> &RepositoryDefinition;
    fn resource(&self) -> &Path;
}

impl<T: Task> Task for &T {
    fn repository(&self) -> &RepositoryDefinition {
        (**self).repository()
    }

    fn resource(&self) -> &Path {
        (**self).resource()
    }
}

pub(crate) fn run_check<P, D>(
    events: &impl EventSink<P>,
    phase: P,
    repositories: &[&RepositoryDefinition],
    parallelism: usize,
    check: impl Fn(&RepositoryDefinition) -> Result<D, AppError> + Sync,
) -> Result<(Vec<D>, Summary), AppError>
where
    P: Copy + Send + Sync,
    D: Send,
{
    events.emit(Event::PhaseStarted { phase, total: repositories.len() })?;
    let started = Instant::now();
    let results = match workers::map(repositories, parallelism, |repository| {
        instrumented(events, phase, repository.display_path(), || check(repository))
    }) {
        Ok(results) => results,
        Err(error) => return phase_failed(events, phase, error),
    };
    let elapsed = started.elapsed();

    let decisions = match results.into_iter().collect::<Result<Vec<D>, AppError>>() {
        Ok(decisions) => decisions,
        Err(error) => return phase_failed(events, phase, error),
    };

    let summary = Summary::new(decisions.len(), elapsed);
    if let Err(error) = events.emit(Event::PhaseCompleted { phase, summary }) {
        return phase_failed(events, phase, error);
    }
    Ok((decisions, summary))
}

pub(crate) fn run_workers<P, T, R>(
    events: &impl EventSink<P>,
    phase: P,
    tasks: &[T],
    parallelism: usize,
    action: impl Fn(&T) -> Result<R, AppError> + Sync,
    changed: impl Fn(&R) -> bool,
) -> Result<(Vec<R>, Summary), AppError>
where
    P: Copy + Send + Sync,
    T: Task + Sync,
    R: Send,
{
    if tasks.is_empty() {
        return Ok((Vec::new(), Summary::default()));
    }

    events.emit(Event::PhaseStarted { phase, total: tasks.len() })?;
    let started = Instant::now();
    let results = match workers::map_keyed(
        tasks,
        parallelism,
        |task| task.resource(),
        |task| instrumented(events, phase, task.repository().display_path(), || action(task)),
    ) {
        Ok(results) => results,
        Err(error) => return phase_failed(events, phase, error),
    };
    let results = match results.into_iter().collect::<Result<Vec<R>, AppError>>() {
        Ok(results) => results,
        Err(error) => return phase_failed(events, phase, error),
    };
    let elapsed = started.elapsed();

    let count = results.iter().filter(|result| changed(result)).count();
    let summary = Summary::new(count, elapsed);
    if let Err(error) = events.emit(Event::PhaseCompleted { phase, summary }) {
        return phase_failed(events, phase, error);
    }
    Ok((results, summary))
}

/// Runs `items` one at a time in order, for work that must not overlap at
/// all, under the same event envelope and error precedence as `run_workers`.
/// `label` names each item in its repository events.
pub(crate) fn run_sequential<P, T, R>(
    events: &impl EventSink<P>,
    phase: P,
    items: &[T],
    label: impl Fn(&T) -> &str,
    mut action: impl FnMut(&T) -> Result<R, AppError>,
    changed: impl Fn(&R) -> bool,
) -> Result<(Vec<R>, Summary), AppError>
where
    P: Copy,
{
    if items.is_empty() {
        return Ok((Vec::new(), Summary::default()));
    }

    events.emit(Event::PhaseStarted { phase, total: items.len() })?;
    let started = Instant::now();
    let mut results = Vec::with_capacity(items.len());
    for item in items {
        match instrumented(events, phase, label(item), || action(item)) {
            Ok(result) => results.push(result),
            Err(error) => return phase_failed(events, phase, error),
        }
    }
    let elapsed = started.elapsed();

    let count = results.iter().filter(|result| changed(result)).count();
    let summary = Summary::new(count, elapsed);
    if let Err(error) = events.emit(Event::PhaseCompleted { phase, summary }) {
        return phase_failed(events, phase, error);
    }
    Ok((results, summary))
}

/// Wrap one repository action in its started/finished event pair. The action's
/// own error stays primary over a failure to emit the finished event.
fn instrumented<P: Copy, R>(
    events: &impl EventSink<P>,
    phase: P,
    repository: &str,
    action: impl FnOnce() -> Result<R, AppError>,
) -> Result<R, AppError> {
    events.emit(Event::RepositoryStarted { repository: repository.to_string(), phase })?;
    let result = action();
    let finished =
        events.emit(Event::RepositoryFinished { repository: repository.to_string(), phase });
    match (result, finished) {
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        (Ok(result), Ok(())) => Ok(result),
    }
}

fn phase_failed<P: Copy, T>(
    events: &impl EventSink<P>,
    phase: P,
    error: AppError,
) -> Result<T, AppError> {
    let _ = events.emit(Event::PhaseFailed { phase });
    Err(error)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Mutex;

    use tempfile::TempDir;

    use super::{Task, run_check, run_sequential, run_workers};
    use crate::AppError;
    use crate::phases::{Event, EventSink};
    use crate::repositories::{RemoteUrl, RepositoryDefinition, RepositoryName};

    #[derive(Clone, Copy)]
    enum Phase {
        Work,
    }

    #[derive(Default)]
    struct RecordingEvents {
        events: Mutex<Vec<&'static str>>,
        reject_failure: bool,
    }

    impl EventSink<Phase> for RecordingEvents {
        fn emit(&self, event: Event<Phase>) -> Result<(), AppError> {
            let name = match event {
                Event::PhaseStarted { .. } => "phase-started",
                Event::RepositoryStarted { .. } => "repository-started",
                Event::GitProgress { .. } => "git-progress",
                Event::RepositoryFinished { .. } => "repository-finished",
                Event::PhaseCompleted { .. } => "phase-completed",
                Event::PhaseFailed { .. } => "phase-failed",
            };
            self.events.lock().unwrap().push(name);
            if self.reject_failure && name == "phase-failed" {
                Err(AppError::internal("failure event rejected"))
            } else {
                Ok(())
            }
        }
    }

    struct TestTask<'a> {
        repository: &'a RepositoryDefinition,
    }

    impl Task for TestTask<'_> {
        fn repository(&self) -> &RepositoryDefinition {
            self.repository
        }

        fn resource(&self) -> &Path {
            self.repository.path()
        }
    }

    #[test]
    fn returned_action_error_terminates_the_phase_and_remains_primary() {
        let root = TempDir::new().unwrap();
        let repository = repository(&root);
        let events = RecordingEvents { events: Mutex::default(), reject_failure: true };
        let tasks = [TestTask { repository: &repository }];

        let result = run_workers(
            &events,
            Phase::Work,
            &tasks,
            1,
            |_| Err::<(), _>(AppError::invalid_arguments("primary failure")),
            |_| false,
        );

        assert!(result.is_err_and(|error| error.to_string() == "primary failure"));
        assert_eq!(
            *events.events.lock().unwrap(),
            ["phase-started", "repository-started", "repository-finished", "phase-failed",]
        );
    }

    #[test]
    fn panicking_check_worker_terminates_the_phase() {
        let root = TempDir::new().unwrap();
        let repository = repository(&root);
        let events = RecordingEvents::default();
        let repositories = [&repository];

        let result =
            run_check(&events, Phase::Work, &repositories, 1, |_| -> Result<(), AppError> {
                panic!("worker panic")
            });

        assert!(result.is_err_and(|error| error.to_string().contains("worker panicked")));
        assert_eq!(
            *events.events.lock().unwrap(),
            ["phase-started", "repository-started", "phase-failed"]
        );
    }

    #[test]
    fn sequential_action_error_remains_primary_over_a_rejected_failure_event() {
        let events = RecordingEvents { events: Mutex::default(), reject_failure: true };
        let mut ran = Vec::new();

        let result = run_sequential(
            &events,
            Phase::Work,
            &["first", "second", "third"],
            |item| item,
            |item| {
                ran.push(*item);
                if *item == "second" {
                    Err(AppError::invalid_arguments("primary failure"))
                } else {
                    Ok(())
                }
            },
            |_| false,
        );

        assert!(result.is_err_and(|error| error.to_string() == "primary failure"));
        assert_eq!(ran, ["first", "second"]);
        assert_eq!(
            *events.events.lock().unwrap(),
            [
                "phase-started",
                "repository-started",
                "repository-finished",
                "repository-started",
                "repository-finished",
                "phase-failed",
            ]
        );
    }

    #[test]
    fn sequential_event_failures_close_the_phase() {
        for rejected in ["repository-started", "repository-finished", "phase-completed"] {
            let events = RejectingEvents { rejected, events: Mutex::default() };

            let result =
                run_sequential(&events, Phase::Work, &["only"], |item| item, |_| Ok(()), |_| false);

            assert!(result.is_err_and(|error| error.to_string() == rejected));
            assert_eq!(events.events.lock().unwrap().last(), Some(&"phase-failed"), "{rejected}");
        }
    }

    /// Rejects one event kind; every emission, rejected or not, is recorded.
    struct RejectingEvents {
        rejected: &'static str,
        events: Mutex<Vec<&'static str>>,
    }

    impl EventSink<Phase> for RejectingEvents {
        fn emit(&self, event: Event<Phase>) -> Result<(), AppError> {
            let name = match event {
                Event::PhaseStarted { .. } => "phase-started",
                Event::RepositoryStarted { .. } => "repository-started",
                Event::GitProgress { .. } => "git-progress",
                Event::RepositoryFinished { .. } => "repository-finished",
                Event::PhaseCompleted { .. } => "phase-completed",
                Event::PhaseFailed { .. } => "phase-failed",
            };
            self.events.lock().unwrap().push(name);
            if name == self.rejected { Err(AppError::invalid_arguments(name)) } else { Ok(()) }
        }
    }

    fn repository(root: &TempDir) -> RepositoryDefinition {
        let path = root.path().join("repository");
        std::fs::create_dir(&path).unwrap();
        RepositoryDefinition::new(
            RepositoryName::new("repository").unwrap(),
            path,
            "repository".to_string(),
            RemoteUrl::new("https://example.com/repository.git").unwrap(),
            None,
            root.path().join("grove.toml"),
            root.path().to_path_buf(),
        )
    }
}
