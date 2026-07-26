//! Shared phase skeleton for the parallel repository use cases.
//!
//! Each use case supplies its own phase marker, per-repository action, and
//! change predicate; this module owns the event envelope, timing, bounded
//! parallel execution, and summary production common to every phase.

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
        emit_repository_started(events, repository, phase)?;
        let result = check(repository);
        match (result, emit_repository_finished(events, repository, phase)) {
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            (Ok(decision), Ok(())) => Ok(decision),
        }
    }) {
        Ok(results) => results,
        Err(error) => return phase_failed(events, phase, error),
    };
    let elapsed = started.elapsed();

    let mut decisions = Vec::with_capacity(results.len());
    for result in results {
        match result {
            Ok(decision) => decisions.push(decision),
            Err(error) => {
                return phase_failed(events, phase, error);
            }
        }
    }

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
        |task| task.resource().to_path_buf(),
        |task| {
            emit_repository_started(events, task.repository(), phase)?;
            let result = action(task);
            match (result, emit_repository_finished(events, task.repository(), phase)) {
                (Err(error), _) | (Ok(_), Err(error)) => Err(error),
                (Ok(result), Ok(())) => Ok(result),
            }
        },
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

fn phase_failed<P: Copy, T>(
    events: &impl EventSink<P>,
    phase: P,
    error: AppError,
) -> Result<T, AppError> {
    let _ = events.emit(Event::PhaseFailed { phase });
    Err(error)
}

fn emit_repository_started<P: Copy>(
    events: &impl EventSink<P>,
    repository: &RepositoryDefinition,
    phase: P,
) -> Result<(), AppError> {
    events
        .emit(Event::RepositoryStarted { repository: repository.display_path().to_string(), phase })
}

fn emit_repository_finished<P: Copy>(
    events: &impl EventSink<P>,
    repository: &RepositoryDefinition,
    phase: P,
) -> Result<(), AppError> {
    events.emit(Event::RepositoryFinished {
        repository: repository.display_path().to_string(),
        phase,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Mutex;

    use tempfile::TempDir;

    use super::{Task, run_check, run_workers};
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
