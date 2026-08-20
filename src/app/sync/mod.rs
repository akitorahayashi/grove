use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::AppError;
use crate::app::AppContext;
use crate::cache::Store;
use crate::config;
use crate::git::GitClient;
use crate::phases::{self, DiscardEvents, EventProgress, EventSink, Slots, Task as PhaseTask};
use crate::repositories::{RepositoryDefinition, select_repositories};

mod check;
mod prepare;
mod report;
mod update;
mod zoxide;

pub(crate) use crate::app::entry::BlockedReasonDetails;
pub use crate::phases::Summary as PhaseSummary;
pub use report::{
    BlockedReason, Outcome, PhaseSummaries, Plan, Report, SkippedReason, ZoxideEntry,
    ZoxideOutcome, ZoxideReport,
};

pub type Entry = crate::app::entry::Entry<Outcome>;

/// One existing repository whose objects can seed the cache entry for its
/// identity. A candidate descending from the update phase carries the common
/// directory its check phase already resolved; one born from an untouched
/// repository derives it when seeding runs.
struct SeedCandidate<'a> {
    index: usize,
    repository: &'a RepositoryDefinition,
    common_directory: Option<PathBuf>,
}

/// The seed sources sharing one repository identity, tried in order until one
/// seeds the entry, so a failing source never suppresses a working sibling.
struct SeedTask<'a> {
    candidates: Vec<SeedCandidate<'a>>,
}

impl PhaseTask for SeedTask<'_> {
    fn repository(&self) -> &RepositoryDefinition {
        self.candidates[0].repository
    }

    fn resource(&self) -> &Path {
        self.candidates[0].repository.path()
    }
}

/// The result of seeding one repository identity: whether an entry now exists,
/// and a note per candidate whose seeding failed.
struct SeedOutcome {
    seeded: bool,
    warnings: Vec<(usize, String)>,
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
    let config = config::load(config_path)?;
    let repositories = select_repositories(config.repositories(), targets)?;
    let parallelism = std::thread::available_parallelism()?.get();
    let started = Instant::now();
    let mut entries = Slots::new(repositories.len());

    let (decisions, checked) =
        check_phase(ctx.git(), &repositories, parallelism, options.dry_run(), events)?;

    let mut preparations = Vec::new();
    let mut seeds = Vec::new();
    for (index, (repository, decision)) in repositories.iter().copied().zip(decisions).enumerate() {
        match decision {
            check::Decision::Entry(entry) => entries.fill(index, entry),
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
                entries.fill(index, entry);
                seeds.push((index, None));
            }
        }
    }

    let needs_cache = !options.dry_run() && (!preparations.is_empty() || !seeds.is_empty());
    let cache = needs_cache.then(|| ctx.cache()).transpose()?;
    let (updates, prepared) =
        prepare_phase(ctx.git(), cache, &preparations, &mut entries, parallelism, events)?;
    let updated = update_phase(ctx.git(), &updates, &mut entries, parallelism, events)?;

    // Repositories that reached the update phase fetched successfully, so their
    // remote is reachable and can be seeded; this covers cleanly updated,
    // up-to-date, and diverged repositories alike. They lead their identity's
    // candidate list so seeding prefers a transport that just proved reachable
    // over an untouched dirty or detached sibling.
    let mut candidates = updates
        .iter()
        .map(|task| (task.index(), Some(task.common_directory().to_path_buf())))
        .collect::<Vec<_>>();
    candidates.extend(seeds);
    let seeded = if let Some(cache) = cache {
        seed_phase(ctx.git(), cache, &repositories, candidates, &mut entries, parallelism, events)?
    } else {
        PhaseSummary::default()
    };

    let entries = entries.into_complete()?;
    let zoxide = if options.register_zoxide() {
        if options.dry_run() {
            Some(zoxide::dry_run(&repositories, &entries))
        } else {
            Some(zoxide::register(ctx.zoxide(), &repositories, &entries))
        }
    } else {
        None
    };
    let phases = PhaseSummaries::new(checked, prepared, updated, seeded);
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
    cache: Option<&Store>,
    tasks: &[prepare::Task<'a>],
    entries: &mut Slots<Entry>,
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
            prepare::Completion::Entry { index, entry, .. } => entries.fill(index, entry),
            prepare::Completion::Update(task) => updates.push(task),
        }
    }
    Ok((updates, summary))
}

fn update_phase(
    git: &impl GitClient,
    tasks: &[update::Task<'_>],
    entries: &mut Slots<Entry>,
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
        entries.fill(index, entry);
    }
    Ok(summary)
}

/// Seed the clone cache from existing repositories that are not yet cached, so
/// later clones of the same URL borrow their objects. Seeding runs after the
/// outcomes are known and is best-effort: a failure is surfaced as a note on
/// the already-final entry, never as a repository failure. Repositories left
/// untouched for a dirty working tree or detached HEAD are seeded here too,
/// since seeding reads only the object store. Each distinct repository
/// identity is seeded once, from its first working candidate.
fn seed_phase(
    git: &impl GitClient,
    cache: &Store,
    repositories: &[&RepositoryDefinition],
    candidates: Vec<(usize, Option<PathBuf>)>,
    entries: &mut Slots<Entry>,
    parallelism: usize,
    events: &impl EventSink<Phase>,
) -> Result<PhaseSummary, AppError> {
    // One task per distinct repository identity — the key cache entries use —
    // and only for identities not already cached, so an all-cached run resolves
    // no seed sources and skips the phase entirely. Same-identity candidates
    // stay on the task as fallback sources.
    let mut task_by_identity = HashMap::<String, usize>::new();
    let mut tasks: Vec<SeedTask<'_>> = Vec::new();
    for (index, common_directory) in candidates {
        let repository = repositories[index];
        if cache.is_cached(repository.url()) {
            continue;
        }
        let candidate = SeedCandidate { index, repository, common_directory };
        match task_by_identity.get(&repository.url().identity()) {
            Some(&task) => tasks[task].candidates.push(candidate),
            None => {
                task_by_identity.insert(repository.url().identity(), tasks.len());
                tasks.push(SeedTask { candidates: vec![candidate] });
            }
        }
    }

    let (outcomes, summary) = phases::run_workers(
        events,
        Phase::Seeding,
        &tasks,
        parallelism,
        |task| seed_repository(git, cache, task, events),
        |outcome| outcome.seeded,
    )?;

    for outcome in outcomes {
        for (index, message) in outcome.warnings {
            if let Some(entry) = entries.get_mut(index) {
                entry.set_warning(message);
            }
        }
    }
    Ok(summary)
}

/// Seeding failures become a note on the already-final entry; only a genuine
/// internal error propagates, matching the prepare phase's error taxonomy. A
/// failed candidate keeps its note even when a sibling seeds the entry.
fn seed_repository(
    git: &impl GitClient,
    cache: &Store,
    task: &SeedTask<'_>,
    events: &impl EventSink<Phase>,
) -> Result<SeedOutcome, AppError> {
    let mut warnings = Vec::new();
    for candidate in &task.candidates {
        let mut progress = EventProgress::new(candidate.repository, events);
        let source = match &candidate.common_directory {
            Some(source) => source.clone(),
            None => match git.common_directory(candidate.repository.path()) {
                Ok(source) => source,
                Err(error) => {
                    warnings.push((candidate.index, error.demote()?));
                    continue;
                }
            },
        };
        match cache.seed_from_local(git, candidate.repository.url(), &source, &mut progress) {
            Ok(_) => return Ok(SeedOutcome { seeded: true, warnings }),
            Err(error) => warnings.push((candidate.index, error.demote()?)),
        }
    }
    Ok(SeedOutcome { seeded: false, warnings })
}
