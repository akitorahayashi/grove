//! Shared phase skeleton for the parallel repository use cases.
//!
//! Each use case supplies its own phase marker, per-repository action, and
//! change predicate; this module owns the event envelope, timing, bounded
//! parallel execution, and summary production common to every phase.

use std::path::Path;
use std::time::Instant;

use crate::AppError;
use crate::app::events::{Event, EventSink, PhaseSummary};
use crate::app::workers;
use crate::repositories::RepositoryDefinition;

/// A unit of parallel repository work: which repository it concerns and which
/// Git resource (common directory) serializes it against sibling worktrees.
pub(crate) trait PhaseTask {
    fn repository(&self) -> &RepositoryDefinition;
    fn resource(&self) -> &Path;
}

impl<T: PhaseTask> PhaseTask for &T {
    fn repository(&self) -> &RepositoryDefinition {
        (**self).repository()
    }

    fn resource(&self) -> &Path {
        (**self).resource()
    }
}

pub(crate) fn run_check_phase<P, D>(
    events: &impl EventSink<P>,
    phase: P,
    repositories: &[&RepositoryDefinition],
    parallelism: usize,
    check: impl Fn(&RepositoryDefinition) -> Result<D, AppError> + Sync,
) -> Result<(Vec<D>, PhaseSummary), AppError>
where
    P: Copy + Send + Sync,
    D: Send,
{
    events.emit(Event::PhaseStarted { phase, total: repositories.len() })?;
    let started = Instant::now();
    let results = workers::map(repositories, parallelism, |repository| {
        emit_repository_started(events, repository, phase)?;
        let result = check(repository);
        emit_repository_finished(events, repository, phase)?;
        result
    })?;
    let elapsed = started.elapsed();

    let mut decisions = Vec::with_capacity(results.len());
    for result in results {
        match result {
            Ok(decision) => decisions.push(decision),
            Err(error) => {
                events.emit(Event::PhaseFailed { phase })?;
                return Err(error);
            }
        }
    }

    let summary = PhaseSummary::new(decisions.len(), elapsed);
    events.emit(Event::PhaseCompleted { phase, summary })?;
    Ok((decisions, summary))
}

pub(crate) fn run_worker_phase<P, T, R>(
    events: &impl EventSink<P>,
    phase: P,
    tasks: &[T],
    parallelism: usize,
    action: impl Fn(&T) -> Result<R, AppError> + Sync,
    changed: impl Fn(&R) -> bool,
) -> Result<(Vec<R>, PhaseSummary), AppError>
where
    P: Copy + Send + Sync,
    T: PhaseTask + Sync,
    R: Send,
{
    if tasks.is_empty() {
        return Ok((Vec::new(), PhaseSummary::default()));
    }

    events.emit(Event::PhaseStarted { phase, total: tasks.len() })?;
    let started = Instant::now();
    let results = workers::map_keyed(
        tasks,
        parallelism,
        |task| task.resource().to_path_buf(),
        |task| {
            emit_repository_started(events, task.repository(), phase)?;
            let result = action(task);
            emit_repository_finished(events, task.repository(), phase)?;
            result
        },
    )?;
    let results = results.into_iter().collect::<Result<Vec<R>, AppError>>()?;
    let elapsed = started.elapsed();

    let count = results.iter().filter(|result| changed(result)).count();
    let summary = PhaseSummary::new(count, elapsed);
    events.emit(Event::PhaseCompleted { phase, summary })?;
    Ok((results, summary))
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
