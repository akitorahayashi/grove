use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::AppError;
use crate::app::AppContext;
use crate::config;
use crate::git::GitClient;
use crate::phases::{self, DiscardEvents, EventSink, Slots, Task as PhaseTask};
use crate::repositories::{RepositoryDefinition, select_repositories};

mod check;
mod fetch;
mod report;
mod task;
mod update;

use task::Task;

pub(crate) use crate::app::entry::BlockedReasonDetails;
pub use crate::phases::Summary as PhaseSummary;
pub use report::{BlockedReason, Outcome, PhaseSummaries, Plan, Report, SkippedReason};

pub type Entry = crate::app::entry::Entry<Outcome>;

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
    let mut entries = Slots::new(repositories.len());

    let (decisions, checked) =
        check_phase(ctx.git(), &repositories, parallelism, options.dry_run(), events)?;

    let mut fetches = Vec::new();
    let mut dry_runs = Vec::new();
    for (index, (repository, decision)) in repositories.iter().copied().zip(decisions).enumerate() {
        match decision {
            check::Decision::Entry(entry) => entries.fill(index, entry),
            check::Decision::Fetch { common_directory, default_branch } => {
                fetches.push(Task::new(index, repository, common_directory, default_branch));
            }
            check::Decision::DryRun { common_directory, default_branch } => {
                dry_runs.push(Task::new(index, repository, common_directory, default_branch));
            }
        }
    }

    plan_dry_runs(&dry_runs, &mut entries);
    let (refreshes, fetched) = fetch_phase(ctx.git(), &fetches, &mut entries, parallelism, events)?;
    let refreshed = refresh_phase(ctx.git(), &refreshes, &mut entries, parallelism, events)?;

    let phases = PhaseSummaries::new(checked, fetched, refreshed);
    Ok(Report::new(entries.into_complete()?, started.elapsed(), phases))
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

fn fetch_phase<'a>(
    git: &impl GitClient,
    tasks: &[Task<'a>],
    entries: &mut Slots<Entry>,
    parallelism: usize,
    events: &impl EventSink<Phase>,
) -> Result<(Vec<Task<'a>>, PhaseSummary), AppError> {
    let (completions, summary) = phases::run_workers(
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
            fetch::Completion::Entry { index, entry } => entries.fill(index, entry),
            fetch::Completion::Refresh(task) => refreshes.push(task),
        }
    }
    Ok((refreshes, summary))
}

fn refresh_phase(
    git: &impl GitClient,
    tasks: &[Task<'_>],
    entries: &mut Slots<Entry>,
    parallelism: usize,
    events: &impl EventSink<Phase>,
) -> Result<PhaseSummary, AppError> {
    let tasks = refreshable_tasks(tasks, entries);
    let (outcomes, summary) = phases::run_workers(
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
        entries.fill(index, entry);
    }
    Ok(summary)
}

fn refreshable_tasks<'a, 'b>(
    tasks: &'b [Task<'a>],
    entries: &mut Slots<Entry>,
) -> Vec<&'b Task<'a>> {
    let conflicts = linked_worktree_conflicts(tasks);

    let mut refreshable = Vec::new();
    for task in tasks {
        if conflicts.contains(&worktree_branch_key(task)) {
            entries.fill(
                task.index(),
                Entry::new(task.repository(), linked_worktree_conflict_outcome(task)),
            );
        } else {
            refreshable.push(task);
        }
    }
    refreshable
}

fn plan_dry_runs(tasks: &[Task<'_>], entries: &mut Slots<Entry>) {
    let conflicts = linked_worktree_conflicts(tasks);

    for task in tasks {
        let outcome = if conflicts.contains(&worktree_branch_key(task)) {
            linked_worktree_conflict_outcome(task)
        } else {
            Outcome::Planned(Plan::new(task.default_branch().to_string()))
        };
        entries.fill(task.index(), Entry::new(task.repository(), outcome));
    }
}

/// The keys claimed by more than one selected repository. Git checks a branch
/// out in at most one worktree of a repository, so two linked worktrees sharing
/// a default branch cannot both be put on it.
fn linked_worktree_conflicts(tasks: &[Task<'_>]) -> HashSet<(PathBuf, String)> {
    let mut counts = HashMap::<(PathBuf, String), usize>::new();
    for task in tasks {
        *counts.entry(worktree_branch_key(task)).or_default() += 1;
    }
    counts.into_iter().filter(|(_, count)| *count > 1).map(|(key, _)| key).collect()
}

fn worktree_branch_key(task: &Task<'_>) -> (PathBuf, String) {
    (task.resource().to_path_buf(), task.default_branch().to_string())
}

fn linked_worktree_conflict_outcome(task: &Task<'_>) -> Outcome {
    Outcome::Blocked {
        reason: BlockedReason::LinkedWorktreeDefaultBranchConflict {
            branch: task.default_branch().to_string(),
        },
    }
}
