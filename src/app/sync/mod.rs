use std::path::Path;
use std::time::Instant;

use crate::AppError;
use crate::app::AppContext;
use crate::config;
use crate::git::GitClient;
use crate::repositories::{RepositoryDefinition, select_repositories};

mod check;
mod events;
mod prepare;
mod report;
mod update;
mod workers;
mod zoxide;

pub use events::Phase;
pub(crate) use events::{Event, EventSink};
pub(crate) use report::BlockedReasonDetails;
pub use report::{
    BlockedReason, Entry, Outcome, PhaseSummaries, PhaseSummary, Plan, Report, SkippedReason,
    ZoxideEntry, ZoxideOutcome, ZoxideReport,
};

use events::DiscardEvents;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncOptions {
    dry_run: bool,
    register_zoxide: bool,
}

impl SyncOptions {
    pub fn new(dry_run: bool, register_zoxide: bool) -> Self {
        Self { dry_run, register_zoxide }
    }

    pub fn dry_run(self) -> bool {
        self.dry_run
    }

    pub fn register_zoxide(self) -> bool {
        self.register_zoxide
    }
}

pub fn execute(
    ctx: &AppContext<impl GitClient, impl crate::zoxide::ZoxideClient>,
    config_path: Option<&Path>,
    targets: &[String],
    dry_run: bool,
) -> Result<Report, AppError> {
    execute_with_options(ctx, config_path, targets, SyncOptions::new(dry_run, false))
}

pub fn execute_with_options(
    ctx: &AppContext<impl GitClient, impl crate::zoxide::ZoxideClient>,
    config_path: Option<&Path>,
    targets: &[String],
    options: SyncOptions,
) -> Result<Report, AppError> {
    execute_with_events(ctx, config_path, targets, options, &DiscardEvents)
}

pub(crate) fn execute_with_events(
    ctx: &AppContext<impl GitClient, impl crate::zoxide::ZoxideClient>,
    config_path: Option<&Path>,
    targets: &[String],
    options: SyncOptions,
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

    let mut preparations = Vec::new();
    for (index, (repository, decision)) in repositories.iter().copied().zip(decisions).enumerate() {
        match decision {
            check::Decision::Entry(entry) => entries[index] = Some(entry),
            check::Decision::Clone => {
                preparations.push(prepare::Task::Clone { index, repository });
            }
            check::Decision::Fetch { common_directory, default_branch } => {
                preparations.push(prepare::Task::Fetch {
                    index,
                    repository,
                    common_directory,
                    default_branch,
                });
            }
        }
    }

    let (updates, prepared) =
        prepare_phase(ctx.git(), &preparations, &mut entries, parallelism, events)?;
    let updated = update_phase(ctx.git(), &updates, &mut entries, parallelism, events)?;

    let entries = entries
        .into_iter()
        .map(|entry| {
            entry.ok_or_else(|| AppError::internal("selected repository produced no outcome"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let zoxide = if options.register_zoxide() {
        if options.dry_run() {
            Some(zoxide::dry_run(&repositories, &entries))
        } else {
            Some(zoxide::register(ctx.zoxide(), &repositories, &entries))
        }
    } else {
        None
    };
    let phases = PhaseSummaries::new(checked, prepared, updated);
    Ok(Report::new(entries, started.elapsed(), phases, zoxide))
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
            Err(err) => {
                events.emit(Event::PhaseFailed { phase: Phase::Checking })?;
                return Err(err);
            }
        }
    }

    let summary = PhaseSummary::new(decisions.len(), elapsed);
    events.emit(Event::PhaseCompleted { phase: Phase::Checking, summary })?;
    Ok((decisions, summary))
}

fn prepare_phase<'a>(
    git: &impl GitClient,
    tasks: &[prepare::Task<'a>],
    entries: &mut [Option<Entry>],
    parallelism: usize,
    events: &impl EventSink,
) -> Result<(Vec<update::Task<'a>>, PhaseSummary), AppError> {
    if tasks.is_empty() {
        return Ok((Vec::new(), PhaseSummary::default()));
    }

    events.emit(Event::PhaseStarted { phase: Phase::Preparing, total: tasks.len() })?;
    let started = Instant::now();
    let completions = workers::map_keyed(
        tasks,
        parallelism,
        |task| task.resource().to_path_buf(),
        |task| {
            emit_repository_started(events, task.repository(), Phase::Preparing)?;
            let completion = prepare::repository(git, task, events);
            emit_repository_finished(events, task.repository(), Phase::Preparing)?;
            completion
        },
    )?;
    let completions = completions.into_iter().collect::<Result<Vec<_>, AppError>>()?;
    let elapsed = started.elapsed();
    let prepared = completions.iter().filter(|completion| completion.prepared()).count();
    let mut updates = Vec::new();

    for completion in completions {
        match completion {
            prepare::Completion::Entry { index, entry, .. } => entries[index] = Some(entry),
            prepare::Completion::Update(task) => updates.push(task),
        }
    }

    let summary = PhaseSummary::new(prepared, elapsed);
    events.emit(Event::PhaseCompleted { phase: Phase::Preparing, summary })?;
    Ok((updates, summary))
}

fn update_phase(
    git: &impl GitClient,
    tasks: &[update::Task<'_>],
    entries: &mut [Option<Entry>],
    parallelism: usize,
    events: &impl EventSink,
) -> Result<PhaseSummary, AppError> {
    if tasks.is_empty() {
        return Ok(PhaseSummary::default());
    }

    events.emit(Event::PhaseStarted { phase: Phase::Updating, total: tasks.len() })?;
    let started = Instant::now();
    let outcomes = workers::map_keyed(
        tasks,
        parallelism,
        |task| task.resource().to_path_buf(),
        |task| {
            emit_repository_started(events, task.repository(), Phase::Updating)?;
            let entry = update::repository(git, task);
            emit_repository_finished(events, task.repository(), Phase::Updating)?;
            Ok((task.index(), entry))
        },
    )?;
    let outcomes = outcomes.into_iter().collect::<Result<Vec<_>, AppError>>()?;
    let elapsed = started.elapsed();
    let updated = outcomes
        .iter()
        .filter(|(_, entry)| {
            matches!(
                entry.outcome(),
                Outcome::Updated { .. } | Outcome::UpdatedButRestorationFailed { .. }
            )
        })
        .count();

    for (index, entry) in outcomes {
        entries[index] = Some(entry);
    }

    let summary = PhaseSummary::new(updated, elapsed);
    events.emit(Event::PhaseCompleted { phase: Phase::Updating, summary })?;
    Ok(summary)
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
