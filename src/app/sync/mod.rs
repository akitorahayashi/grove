use std::path::Path;
use std::time::Instant;

use crate::AppError;
use crate::app::AppContext;
use crate::app::cache::CacheStore;
use crate::app::events::{DiscardEvents, EventSink};
use crate::app::phases;
use crate::config;
use crate::git::GitClient;
use crate::repositories::{RepositoryDefinition, select_repositories};

mod check;
mod prepare;
mod report;
mod update;
mod zoxide;

pub use crate::app::events::PhaseSummary;
pub(crate) use crate::app::report::BlockedReasonDetails;
pub use report::{
    BlockedReason, Outcome, PhaseSummaries, Plan, Report, SkippedReason, ZoxideEntry,
    ZoxideOutcome, ZoxideReport,
};

pub type Entry = crate::app::report::Entry<Outcome>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Checking,
    Preparing,
    Updating,
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
    let cache = CacheStore::from_env()?;
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
        prepare_phase(ctx.git(), &cache, &preparations, &mut entries, parallelism, events)?;
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
    events: &impl EventSink<Phase>,
) -> Result<(Vec<check::Decision>, PhaseSummary), AppError> {
    phases::run_check_phase(events, Phase::Checking, repositories, parallelism, |repository| {
        check::repository(git, repository, dry_run)
    })
}

fn prepare_phase<'a>(
    git: &impl GitClient,
    cache: &CacheStore,
    tasks: &[prepare::Task<'a>],
    entries: &mut [Option<Entry>],
    parallelism: usize,
    events: &impl EventSink<Phase>,
) -> Result<(Vec<update::Task<'a>>, PhaseSummary), AppError> {
    let (completions, summary) = phases::run_worker_phase(
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
    let (outcomes, summary) = phases::run_worker_phase(
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
