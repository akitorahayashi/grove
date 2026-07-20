use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::AppError;
use crate::app::AppContext;
use crate::app::events::{DiscardEvents, EventSink};
use crate::app::phases::{self, PhaseTask};
use crate::config;
use crate::git::GitClient;
use crate::repositories::{RepositoryDefinition, select_repositories};

mod check;
mod fetch;
mod report;
mod update;

pub use crate::app::events::PhaseSummary;
pub(crate) use report::BlockedReasonDetails;
pub use report::{BlockedReason, Entry, Outcome, PhaseSummaries, Plan, Report, SkippedReason};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Checking,
    Fetching,
    Refreshing,
}

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
    ctx: &AppContext<impl GitClient>,
    config_path: Option<&Path>,
    targets: &[String],
    dry_run: bool,
) -> Result<Report, AppError> {
    execute_with_options(ctx, config_path, targets, RefreshOptions::new(dry_run))
}

pub fn execute_with_options(
    ctx: &AppContext<impl GitClient>,
    config_path: Option<&Path>,
    targets: &[String],
    options: RefreshOptions,
) -> Result<Report, AppError> {
    execute_with_events(ctx, config_path, targets, options, &DiscardEvents)
}

pub(crate) fn execute_with_events(
    ctx: &AppContext<impl GitClient>,
    config_path: Option<&Path>,
    targets: &[String],
    options: RefreshOptions,
    events: &impl EventSink<Phase>,
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
    events: &impl EventSink<Phase>,
) -> Result<(Vec<check::Decision>, PhaseSummary), AppError> {
    phases::run_check_phase(events, Phase::Checking, repositories, parallelism, |repository| {
        check::repository(git, repository, dry_run)
    })
}

fn fetch_phase<'a>(
    git: &impl GitClient,
    tasks: &[fetch::Task<'a>],
    entries: &mut [Option<Entry>],
    parallelism: usize,
    events: &impl EventSink<Phase>,
) -> Result<(Vec<update::Task<'a>>, PhaseSummary), AppError> {
    let (completions, summary) = phases::run_worker_phase(
        events,
        Phase::Fetching,
        tasks,
        parallelism,
        |task| fetch::repository(git, task, events),
        |completion| completion.fetched(),
    )?;

    let mut refreshes = Vec::new();
    for completion in completions {
        match completion {
            fetch::Completion::Entry { index, entry } => entries[index] = Some(entry),
            fetch::Completion::Refresh(task) => refreshes.push(task),
        }
    }
    Ok((refreshes, summary))
}

fn refresh_phase(
    git: &impl GitClient,
    tasks: &[update::Task<'_>],
    entries: &mut [Option<Entry>],
    parallelism: usize,
    events: &impl EventSink<Phase>,
) -> Result<PhaseSummary, AppError> {
    let tasks = refreshable_tasks(tasks, entries);
    let (outcomes, summary) = phases::run_worker_phase(
        events,
        Phase::Refreshing,
        &tasks,
        parallelism,
        |task| Ok((task.index(), update::repository(git, task))),
        |(_, entry)| {
            matches!(
                entry.outcome(),
                Outcome::Refreshed { .. }
                    | Outcome::Switched { .. }
                    | Outcome::SwitchedAndBlocked { .. }
            )
        },
    )?;

    for (index, entry) in outcomes {
        entries[index] = Some(entry);
    }
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
