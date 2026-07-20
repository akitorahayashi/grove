use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::AppError;
use crate::app::{AppContext, workers};
use crate::config;
use crate::git::GitClient;
use crate::repositories::{RepositoryDefinition, select_repositories};

mod check;
mod events;
mod fetch;
mod report;
mod update;

pub use events::Phase;
pub(crate) use events::{Event, EventSink};
pub(crate) use report::BlockedReasonDetails;
pub use report::{
    BlockedReason, Entry, Outcome, PhaseSummaries, PhaseSummary, Plan, Report, SkippedReason,
};

use events::DiscardEvents;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RefreshOptions {
    dry_run: bool,
}

impl RefreshOptions {
    pub fn new(dry_run: bool) -> Self {
        Self { dry_run }
    }

    pub fn dry_run(self) -> bool {
        self.dry_run
    }
}

pub fn execute(
    ctx: &AppContext<impl GitClient, impl crate::zoxide::ZoxideClient>,
    config_path: Option<&Path>,
    targets: &[String],
    dry_run: bool,
) -> Result<Report, AppError> {
    execute_with_options(ctx, config_path, targets, RefreshOptions::new(dry_run))
}

pub fn execute_with_options(
    ctx: &AppContext<impl GitClient, impl crate::zoxide::ZoxideClient>,
    config_path: Option<&Path>,
    targets: &[String],
    options: RefreshOptions,
) -> Result<Report, AppError> {
    execute_with_events(ctx, config_path, targets, options, &DiscardEvents)
}

pub(crate) fn execute_with_events(
    ctx: &AppContext<impl GitClient, impl crate::zoxide::ZoxideClient>,
    config_path: Option<&Path>,
    targets: &[String],
    options: RefreshOptions,
    events: &impl EventSink,
) -> Result<Report, AppError> {
    ctx.git().verify_available()?;
    let config = config::load(config_path)?;
    let repositories = select_repositories(config.repositories(), targets)?;
    let parallelism = std::thread::available_parallelism()?.get();
    let started = Instant::now();
    let total = repositories.len();
    let mut entries = std::iter::repeat_with(|| None).take(total).collect::<Vec<_>>();

    let (decisions, checked) =
        check_phase(ctx.git(), &repositories, parallelism, options.dry_run(), events)?;

    let mut fetches = Vec::new();
    for (index, (repository, decision)) in repositories.iter().copied().zip(decisions).enumerate() {
        match decision {
            check::Decision::Entry(entry) => entries[index] = Some(entry),
            check::Decision::Fetch { common_directory, default_branch } => {
                fetches.push(fetch::Task::new(index, repository, common_directory, default_branch));
            }
        }
    }

    let (refreshes, fetched) = fetch_phase(ctx.git(), &fetches, &mut entries, parallelism, events)?;
    let refreshed = refresh_phase(ctx.git(), &refreshes, &mut entries, parallelism, events)?;

    let entries = entries
        .into_iter()
        .map(|entry| {
            entry.ok_or_else(|| AppError::internal("selected repository produced no outcome"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let phases = PhaseSummaries::new(checked, fetched, refreshed);
    Ok(Report::new(entries, started.elapsed(), phases))
}

fn check_phase(
    git: &impl GitClient,
    repositories: &[&RepositoryDefinition],
    parallelism: usize,
    dry_run: bool,
    events: &impl EventSink,
) -> Result<(Vec<check::Decision>, PhaseSummary), AppError> {
    events.emit(Event::PhaseStarted { phase: Phase::Checking, total: repositories.len() })?;
    let started = Instant::now();
    let results = workers::map(repositories, parallelism, |repository| {
        emit_repository_started(events, repository, Phase::Checking)?;
        let result = check::repository(git, repository, dry_run);
        emit_repository_finished(events, repository, Phase::Checking)?;
        result
    })?;
    let elapsed = started.elapsed();

    let mut decisions = Vec::with_capacity(results.len());
    for result in results {
        match result {
            Ok(decision) => decisions.push(decision),
            Err(error) => {
                events.emit(Event::PhaseFailed { phase: Phase::Checking })?;
                return Err(error);
            }
        }
    }

    let summary = PhaseSummary::new(decisions.len(), elapsed);
    events.emit(Event::PhaseCompleted { phase: Phase::Checking, summary })?;
    Ok((decisions, summary))
}

fn fetch_phase<'a>(
    git: &impl GitClient,
    tasks: &[fetch::Task<'a>],
    entries: &mut [Option<Entry>],
    parallelism: usize,
    events: &impl EventSink,
) -> Result<(Vec<update::Task<'a>>, PhaseSummary), AppError> {
    if tasks.is_empty() {
        return Ok((Vec::new(), PhaseSummary::default()));
    }

    events.emit(Event::PhaseStarted { phase: Phase::Fetching, total: tasks.len() })?;
    let started = Instant::now();
    let completions = workers::map_keyed(
        tasks,
        parallelism,
        |task| task.resource().to_path_buf(),
        |task| {
            emit_repository_started(events, task.repository(), Phase::Fetching)?;
            let completion = fetch::repository(git, task, events);
            emit_repository_finished(events, task.repository(), Phase::Fetching)?;
            completion
        },
    )?;
    let completions = completions.into_iter().collect::<Result<Vec<_>, AppError>>()?;
    let elapsed = started.elapsed();
    let fetched = completions.iter().filter(|completion| completion.fetched()).count();
    let mut refreshes = Vec::new();

    for completion in completions {
        match completion {
            fetch::Completion::Entry { index, entry } => entries[index] = Some(entry),
            fetch::Completion::Refresh(task) => refreshes.push(task),
        }
    }

    let summary = PhaseSummary::new(fetched, elapsed);
    events.emit(Event::PhaseCompleted { phase: Phase::Fetching, summary })?;
    Ok((refreshes, summary))
}

fn refresh_phase(
    git: &impl GitClient,
    tasks: &[update::Task<'_>],
    entries: &mut [Option<Entry>],
    parallelism: usize,
    events: &impl EventSink,
) -> Result<PhaseSummary, AppError> {
    if tasks.is_empty() {
        return Ok(PhaseSummary::default());
    }

    let tasks = refreshable_tasks(tasks, entries);
    if tasks.is_empty() {
        return Ok(PhaseSummary::default());
    }

    events.emit(Event::PhaseStarted { phase: Phase::Refreshing, total: tasks.len() })?;
    let started = Instant::now();
    let outcomes = workers::map_keyed(
        &tasks,
        parallelism,
        |task| task.resource().to_path_buf(),
        |task| {
            emit_repository_started(events, task.repository(), Phase::Refreshing)?;
            let entry = update::repository(git, task);
            emit_repository_finished(events, task.repository(), Phase::Refreshing)?;
            Ok((task.index(), entry))
        },
    )?;
    let outcomes = outcomes.into_iter().collect::<Result<Vec<_>, AppError>>()?;
    let elapsed = started.elapsed();
    let changed = outcomes
        .iter()
        .filter(|(_, entry)| {
            matches!(
                entry.outcome(),
                Outcome::Refreshed { .. }
                    | Outcome::Switched { .. }
                    | Outcome::SwitchedAndBlocked { .. }
            )
        })
        .count();

    for (index, entry) in outcomes {
        entries[index] = Some(entry);
    }

    let summary = PhaseSummary::new(changed, elapsed);
    events.emit(Event::PhaseCompleted { phase: Phase::Refreshing, summary })?;
    Ok(summary)
}

fn refreshable_tasks<'a, 'b>(
    tasks: &'b [update::Task<'a>],
    entries: &mut [Option<Entry>],
) -> Vec<&'b update::Task<'a>> {
    let mut counts = HashMap::<(PathBuf, String), usize>::new();
    for task in tasks {
        let key = (task.resource().to_path_buf(), task.default_branch().to_string());
        *counts.entry(key).or_default() += 1;
    }

    let mut refreshable = Vec::new();
    for task in tasks {
        let key = (task.resource().to_path_buf(), task.default_branch().to_string());
        if counts[&key] > 1 {
            entries[task.index()] = Some(Entry::new(
                task.repository(),
                Outcome::Blocked {
                    reason: BlockedReason::LinkedWorktreeDefaultBranchConflict {
                        branch: task.default_branch().to_string(),
                    },
                },
            ));
        } else {
            refreshable.push(task);
        }
    }
    refreshable
}

fn emit_repository_started(
    events: &impl EventSink,
    repository: &RepositoryDefinition,
    phase: Phase,
) -> Result<(), AppError> {
    events
        .emit(Event::RepositoryStarted { repository: repository.display_path().to_string(), phase })
}

fn emit_repository_finished(
    events: &impl EventSink,
    repository: &RepositoryDefinition,
    phase: Phase,
) -> Result<(), AppError> {
    events.emit(Event::RepositoryFinished {
        repository: repository.display_path().to_string(),
        phase,
    })
}
