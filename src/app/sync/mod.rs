use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

use crate::AppError;
use crate::app::AppContext;
use crate::cache::Store;
use crate::config;
use crate::git::GitClient;
use crate::phases::{self, DiscardEvents, EventProgress, EventSink, Task as PhaseTask};
use crate::repositories::{RepositoryDefinition, select_repositories};

mod check;
mod prepare;
mod report;
mod update;
mod zoxide;

pub(crate) use crate::inspection::BlockedReasonDetails;
pub use crate::phases::Summary as PhaseSummary;
pub use report::{
    BlockedReason, Outcome, PhaseSummaries, Plan, Report, SkippedReason, ZoxideEntry,
    ZoxideOutcome, ZoxideReport,
};

pub type Entry = crate::app::entry::Entry<Outcome>;

/// An existing repository eligible to seed the clone cache from its objects.
struct SeedTask<'a> {
    index: usize,
    repository: &'a RepositoryDefinition,
}

impl PhaseTask for SeedTask<'_> {
    fn repository(&self) -> &RepositoryDefinition {
        self.repository
    }

    fn resource(&self) -> &Path {
        self.repository.path()
    }
}

/// The result of seeding one repository: the note to attach when seeding failed.
struct SeedOutcome {
    index: usize,
    warning: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Checking,
    Preparing,
    Updating,
    Seeding,
}

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
    events: &impl EventSink<Phase>,
) -> Result<Report, AppError> {
    ctx.git().verify_available()?;
    let cache = ctx.cache();
    let config = config::load(config_path)?;
    let repositories = select_repositories(config.repositories(), targets)?;
    let parallelism = std::thread::available_parallelism()?.get();
    let started = Instant::now();
    let total = repositories.len();
    let mut entries = std::iter::repeat_with(|| None).take(total).collect::<Vec<_>>();

    let (decisions, checked) =
        check_phase(ctx.git(), &repositories, parallelism, options.dry_run(), events)?;

    let mut preparations = Vec::new();
    let mut seed_indices = Vec::new();
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
            check::Decision::SeedOnly { entry } => {
                entries[index] = Some(entry);
                seed_indices.push(index);
            }
        }
    }

    let (updates, prepared) =
        prepare_phase(ctx.git(), cache, &preparations, &mut entries, parallelism, events)?;
    let updated = update_phase(ctx.git(), &updates, &mut entries, parallelism, events)?;

    // Repositories that reached the update phase fetched successfully, so their
    // remote is reachable and can be seeded; this covers cleanly updated,
    // up-to-date, and diverged repositories alike.
    seed_indices.extend(updates.iter().map(update::Task::index));
    seed_phase(ctx.git(), cache, &repositories, &seed_indices, &mut entries, parallelism, events)?;

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
    events: &impl EventSink<Phase>,
) -> Result<(Vec<check::Decision>, PhaseSummary), AppError> {
    phases::run_check(events, Phase::Checking, repositories, parallelism, |repository| {
        check::repository(git, repository, dry_run)
    })
}

fn prepare_phase<'a>(
    git: &impl GitClient,
    cache: &Store,
    tasks: &[prepare::Task<'a>],
    entries: &mut [Option<Entry>],
    parallelism: usize,
    events: &impl EventSink<Phase>,
) -> Result<(Vec<update::Task<'a>>, PhaseSummary), AppError> {
    let (completions, summary) = phases::run_workers(
        events,
        Phase::Preparing,
        tasks,
        parallelism,
        |task| prepare::repository(git, cache, task, events),
        |completion| completion.prepared(),
    )?;

    let mut updates = Vec::new();
    for completion in completions {
        match completion {
            prepare::Completion::Entry { index, entry, .. } => entries[index] = Some(entry),
            prepare::Completion::Update(task) => updates.push(task),
        }
    }
    Ok((updates, summary))
}

fn update_phase(
    git: &impl GitClient,
    tasks: &[update::Task<'_>],
    entries: &mut [Option<Entry>],
    parallelism: usize,
    events: &impl EventSink<Phase>,
) -> Result<PhaseSummary, AppError> {
    let (outcomes, summary) = phases::run_workers(
        events,
        Phase::Updating,
        tasks,
        parallelism,
        |task| Ok((task.index(), update::repository(git, task))),
        |(_, entry)| {
            matches!(
                entry.outcome(),
                Outcome::Updated { .. } | Outcome::UpdatedButRestorationFailed { .. }
            )
        },
    )?;

    for (index, entry) in outcomes {
        entries[index] = Some(entry);
    }
    Ok(summary)
}

/// Seed the clone cache from existing repositories that are not yet cached, so
/// later clones of the same URL borrow their objects. Seeding runs after the
/// outcomes are known and is best-effort: a failure is surfaced as a note on
/// the already-final entry, never as a repository failure. Repositories left
/// untouched for a dirty working tree or detached HEAD are seeded here too,
/// since seeding reads only the object store. Each distinct URL is seeded once.
fn seed_phase(
    git: &impl GitClient,
    cache: &Store,
    repositories: &[&RepositoryDefinition],
    indices: &[usize],
    entries: &mut [Option<Entry>],
    parallelism: usize,
    events: &impl EventSink<Phase>,
) -> Result<(), AppError> {
    // One task per distinct URL, and only for URLs not already cached, so an
    // all-cached run resolves no seed sources and skips the phase entirely.
    let mut seen = HashSet::new();
    let tasks = indices
        .iter()
        .filter(|&&index| seen.insert(repositories[index].url().as_process_argument()))
        .filter(|&&index| !cache.is_cached(repositories[index].url()))
        .map(|&index| SeedTask { index, repository: repositories[index] })
        .collect::<Vec<_>>();

    let (outcomes, _summary) = phases::run_workers(
        events,
        Phase::Seeding,
        &tasks,
        parallelism,
        |task| seed_repository(git, cache, task, events),
        |outcome| outcome.warning.is_none(),
    )?;

    for outcome in outcomes {
        if let Some(message) = outcome.warning
            && let Some(entry) = entries[outcome.index].as_mut()
        {
            entry.set_warning(message);
        }
    }
    Ok(())
}

fn seed_repository(
    git: &impl GitClient,
    cache: &Store,
    task: &SeedTask<'_>,
    events: &impl EventSink<Phase>,
) -> Result<SeedOutcome, AppError> {
    let mut progress = EventProgress::new(task.repository, events);
    let source = match git.common_directory(task.repository.path()) {
        Ok(source) => source,
        Err(error) => return demote_seed_failure(task.index, error),
    };
    match cache.seed_from_local(git, task.repository.url(), &source, &mut progress) {
        Ok(_) => Ok(SeedOutcome { index: task.index, warning: None }),
        Err(error) => demote_seed_failure(task.index, error),
    }
}

/// A genuine internal error propagates and aborts the run, matching the prepare
/// phase's error taxonomy; any other seeding failure becomes a note on the entry.
fn demote_seed_failure(index: usize, error: AppError) -> Result<SeedOutcome, AppError> {
    if error.is_internal() {
        Err(error)
    } else {
        Ok(SeedOutcome { index, warning: Some(error.to_string()) })
    }
}
