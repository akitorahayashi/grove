use std::path::Path;
use std::time::{Duration, Instant};

use crate::AppError;
use crate::app::AppContext;
use crate::config;
use crate::git::{GitClient, GitProgress, GitProgressSink, urls_match};
use crate::repositories::{RepositoryDefinition, select_repositories};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Clone { url: String },
    CheckExisting { branch: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Checking,
    Cloning,
    Fetching,
    Updating,
}

impl Phase {
    pub fn message(self) -> &'static str {
        match self {
            Self::Checking => "Checking",
            Self::Cloning => "Cloning",
            Self::Fetching => "Fetching",
            Self::Updating => "Updating",
        }
    }
}

pub trait SyncObserver {
    fn phase_started(&mut self, _repository: &str, _phase: Phase) {}

    fn git_progress(&mut self, _repository: &str, _phase: Phase, _progress: &GitProgress) {}

    fn phase_finished(&mut self, _repository: &str, _phase: Phase) {}
}

#[derive(Debug, Default)]
pub struct NoopObserver;

impl SyncObserver for NoopObserver {}

#[derive(Debug, Clone, Copy, Default)]
pub struct PhaseSummary {
    count: usize,
    elapsed: Duration,
}

impl PhaseSummary {
    fn add(&mut self, elapsed: Duration) {
        self.count += 1;
        self.elapsed += elapsed;
    }

    pub fn count(self) -> usize {
        self.count
    }

    pub fn elapsed(self) -> Duration {
        self.elapsed
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PhaseSummaries {
    checked: PhaseSummary,
    fetched: PhaseSummary,
    cloned: PhaseSummary,
    updated: PhaseSummary,
}

impl PhaseSummaries {
    pub fn checked(self) -> PhaseSummary {
        self.checked
    }

    pub fn fetched(self) -> PhaseSummary {
        self.fetched
    }

    pub fn cloned(self) -> PhaseSummary {
        self.cloned
    }

    pub fn updated(self) -> PhaseSummary {
        self.updated
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkippedReason {
    DirtyWorkingTree,
}

impl SkippedReason {
    pub fn message(&self) -> &str {
        match self {
            Self::DirtyWorkingTree => "dirty working tree",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockedReason {
    DestinationNotGitRepository,
    MissingOrigin,
    RemoteUrlMismatch,
    DetachedHead,
    FetchFailed(String),
    MissingRemoteDefaultBranch,
    MissingLocalBranch { branch: String },
    MissingRemoteBranch { branch: String },
    CannotCompareDefaultBranch,
    Diverged { branch: String },
    AheadOfOrigin { branch: String },
    UpdateFailed(String),
    CloneFailed(String),
}

impl BlockedReason {
    pub fn message(&self) -> String {
        match self {
            Self::DestinationNotGitRepository => {
                "destination exists but is not a Git repository".to_string()
            }
            Self::MissingOrigin => "remote origin is missing".to_string(),
            Self::RemoteUrlMismatch => "remote URL does not match grove.toml".to_string(),
            Self::DetachedHead => "detached HEAD cannot be restored safely".to_string(),
            Self::FetchFailed(message) => message.clone(),
            Self::MissingRemoteDefaultBranch => {
                "remote default branch cannot be determined".to_string()
            }
            Self::MissingLocalBranch { branch } => {
                format!("local default branch '{branch}' is missing")
            }
            Self::MissingRemoteBranch { branch } => {
                format!("remote default branch 'origin/{branch}' is missing")
            }
            Self::CannotCompareDefaultBranch => {
                "default branch cannot be compared with origin".to_string()
            }
            Self::Diverged { branch } => format!("{branch} has diverged"),
            Self::AheadOfOrigin { branch } => {
                format!("{branch} is ahead of origin/{branch}")
            }
            Self::UpdateFailed(message) | Self::CloneFailed(message) => message.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Planned(Plan),
    Cloned { url: String },
    Updated { branch: String, before: String, after: String },
    Current { branch: String },
    Skipped { reason: SkippedReason },
    Blocked { reason: BlockedReason },
}

#[derive(Debug, Clone)]
pub struct Entry {
    repository: String,
    outcome: Outcome,
}

impl Entry {
    fn new(repository: &RepositoryDefinition, outcome: Outcome) -> Self {
        Self { repository: repository.display_path().to_string(), outcome }
    }

    pub fn repository(&self) -> &str {
        &self.repository
    }

    pub fn outcome(&self) -> &Outcome {
        &self.outcome
    }
}

#[derive(Debug, Clone)]
pub struct Report {
    entries: Vec<Entry>,
    elapsed: Duration,
    phases: PhaseSummaries,
}

impl Report {
    pub fn new(entries: Vec<Entry>, elapsed: Duration, phases: PhaseSummaries) -> Self {
        Self { entries, elapsed, phases }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    pub fn phases(&self) -> PhaseSummaries {
        self.phases
    }

    pub fn total(&self) -> usize {
        self.entries.len()
    }

    pub fn planned_clones(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.outcome, Outcome::Planned(Plan::Clone { .. })))
            .count()
    }

    pub fn planned_checks(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.outcome, Outcome::Planned(Plan::CheckExisting { .. })))
            .count()
    }

    pub fn cloned(&self) -> usize {
        self.entries.iter().filter(|entry| matches!(entry.outcome, Outcome::Cloned { .. })).count()
    }

    pub fn updated(&self) -> usize {
        self.entries.iter().filter(|entry| matches!(entry.outcome, Outcome::Updated { .. })).count()
    }

    pub fn skipped(&self) -> usize {
        self.entries.iter().filter(|entry| matches!(entry.outcome, Outcome::Skipped { .. })).count()
    }

    pub fn blocked(&self) -> usize {
        self.entries.iter().filter(|entry| matches!(entry.outcome, Outcome::Blocked { .. })).count()
    }

    pub fn has_failures(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| matches!(entry.outcome, Outcome::Skipped { .. } | Outcome::Blocked { .. }))
    }
}

pub fn execute(
    ctx: &AppContext<impl GitClient>,
    config_path: Option<&Path>,
    targets: &[String],
    dry_run: bool,
) -> Result<Report, AppError> {
    execute_with_observer(ctx, config_path, targets, dry_run, &mut NoopObserver)
}

pub fn execute_with_observer(
    ctx: &AppContext<impl GitClient>,
    config_path: Option<&Path>,
    targets: &[String],
    dry_run: bool,
    observer: &mut impl SyncObserver,
) -> Result<Report, AppError> {
    ctx.git().verify_available()?;
    let config = config::load(config_path)?;
    let repositories = select_repositories(config.repositories(), targets)?;
    let started = Instant::now();
    let mut phases = PhaseSummaries::default();
    let mut entries = Vec::with_capacity(repositories.len());

    for repository in repositories {
        entries.push(sync_repository(ctx.git(), repository, dry_run, observer, &mut phases)?);
    }

    Ok(Report::new(entries, started.elapsed(), phases))
}

fn sync_repository(
    git: &impl GitClient,
    repository: &RepositoryDefinition,
    dry_run: bool,
    observer: &mut impl SyncObserver,
    phases: &mut PhaseSummaries,
) -> Result<Entry, AppError> {
    let checked = time_phase(observer, repository, Phase::Checking, || {
        check_repository(git, repository, dry_run)
    })?;
    phases.checked.add(checked.elapsed);
    let decision = checked.value;

    match decision {
        SyncDecision::Entry(entry) => Ok(entry),
        SyncDecision::Clone => {
            let cloned = time_git_phase(observer, repository, Phase::Cloning, |sink| {
                git.clone_repository(repository.url(), repository.path(), sink)
            });

            match cloned {
                Ok(cloned) => {
                    phases.cloned.add(cloned.elapsed);
                    Ok(Entry::new(
                        repository,
                        Outcome::Cloned { url: repository.url().to_string() },
                    ))
                }
                Err(err) => Ok(Entry::new(
                    repository,
                    Outcome::Blocked { reason: BlockedReason::CloneFailed(err.to_string()) },
                )),
            }
        }
        SyncDecision::Update { default_branch, current_branch } => {
            match time_git_phase(observer, repository, Phase::Fetching, |sink| {
                git.fetch(repository.path(), sink)
            }) {
                Ok(fetched) => phases.fetched.add(fetched.elapsed),
                Err(err) => {
                    return Ok(Entry::new(
                        repository,
                        Outcome::Blocked { reason: BlockedReason::FetchFailed(err.to_string()) },
                    ));
                }
            }

            update_repository(git, repository, default_branch, current_branch, observer, phases)
        }
    }
}

fn check_repository(
    git: &impl GitClient,
    repository: &RepositoryDefinition,
    dry_run: bool,
) -> Result<SyncDecision, AppError> {
    if !repository.path().exists() {
        if dry_run {
            return Ok(SyncDecision::Entry(Entry::new(
                repository,
                Outcome::Planned(Plan::Clone { url: repository.url().to_string() }),
            )));
        }

        return Ok(SyncDecision::Clone);
    }

    if !repository.path().is_dir() || !git.is_work_tree(repository.path())? {
        return Ok(SyncDecision::Entry(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::DestinationNotGitRepository },
        )));
    }

    let Some(actual_url) = git.remote_url(repository.path())? else {
        return Ok(SyncDecision::Entry(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::MissingOrigin },
        )));
    };
    if !urls_match(&actual_url, repository.url()) {
        return Ok(SyncDecision::Entry(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::RemoteUrlMismatch },
        )));
    }

    let Some(current_branch) = git.current_branch(repository.path())? else {
        return Ok(SyncDecision::Entry(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::DetachedHead },
        )));
    };

    if !git.working_tree_clean(repository.path())? {
        return Ok(SyncDecision::Entry(Entry::new(
            repository,
            Outcome::Skipped { reason: SkippedReason::DirtyWorkingTree },
        )));
    }

    if dry_run {
        return plan_existing_repository(git, repository).map(SyncDecision::Entry);
    }

    let Some(default_branch) =
        git.default_branch(repository.path(), repository.default_branch())?
    else {
        return Ok(SyncDecision::Entry(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::MissingRemoteDefaultBranch },
        )));
    };

    Ok(SyncDecision::Update { default_branch, current_branch })
}

fn update_repository(
    git: &impl GitClient,
    repository: &RepositoryDefinition,
    default_branch: String,
    current_branch: String,
    observer: &mut impl SyncObserver,
    phases: &mut PhaseSummaries,
) -> Result<Entry, AppError> {
    if let Some(reason) = default_branch_block_reason(git, repository, &default_branch)? {
        return Ok(Entry::new(repository, Outcome::Blocked { reason }));
    }

    let Some(divergence) = git.branch_divergence(repository.path(), &default_branch)? else {
        return Ok(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::CannotCompareDefaultBranch },
        ));
    };
    if divergence.ahead() > 0 && divergence.behind() > 0 {
        return Ok(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::Diverged { branch: default_branch } },
        ));
    }
    if divergence.ahead() > 0 {
        return Ok(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::AheadOfOrigin { branch: default_branch } },
        ));
    }

    let update = time_phase(observer, repository, Phase::Updating, || {
        git.update_default_branch(repository.path(), &default_branch, &current_branch)
    });
    let update = match update {
        Ok(update) => {
            phases.updated.add(update.elapsed);
            update.value
        }
        Err(err) => {
            return Ok(Entry::new(
                repository,
                Outcome::Blocked { reason: BlockedReason::UpdateFailed(err.to_string()) },
            ));
        }
    };

    if update.changed() {
        Ok(Entry::new(
            repository,
            Outcome::Updated {
                branch: default_branch,
                before: update.before().to_string(),
                after: update.after().to_string(),
            },
        ))
    } else {
        Ok(Entry::new(repository, Outcome::Current { branch: default_branch }))
    }
}

enum SyncDecision {
    Entry(Entry),
    Clone,
    Update { default_branch: String, current_branch: String },
}

struct Timed<T> {
    value: T,
    elapsed: Duration,
}

fn time_phase<T>(
    observer: &mut impl SyncObserver,
    repository: &RepositoryDefinition,
    phase: Phase,
    action: impl FnOnce() -> Result<T, AppError>,
) -> Result<Timed<T>, AppError> {
    observer.phase_started(repository.display_path(), phase);
    let started = Instant::now();
    let value = action();
    let elapsed = started.elapsed();
    observer.phase_finished(repository.display_path(), phase);
    value.map(|value| Timed { value, elapsed })
}

fn time_git_phase<T>(
    observer: &mut impl SyncObserver,
    repository: &RepositoryDefinition,
    phase: Phase,
    action: impl FnOnce(&mut dyn GitProgressSink) -> Result<T, AppError>,
) -> Result<Timed<T>, AppError> {
    observer.phase_started(repository.display_path(), phase);
    let started = Instant::now();
    let value = {
        let mut sink = ObserverProgressSink::new(repository, phase, observer);
        action(&mut sink)
    };
    let elapsed = started.elapsed();
    observer.phase_finished(repository.display_path(), phase);
    value.map(|value| Timed { value, elapsed })
}

struct ObserverProgressSink<'a, O: SyncObserver> {
    repository: &'a RepositoryDefinition,
    phase: Phase,
    observer: &'a mut O,
}

impl<'a, O: SyncObserver> ObserverProgressSink<'a, O> {
    fn new(repository: &'a RepositoryDefinition, phase: Phase, observer: &'a mut O) -> Self {
        Self { repository, phase, observer }
    }
}

impl<O: SyncObserver> GitProgressSink for ObserverProgressSink<'_, O> {
    fn progress(&mut self, progress: GitProgress) {
        self.observer.git_progress(self.repository.display_path(), self.phase, &progress);
    }
}

fn plan_existing_repository(
    git: &impl GitClient,
    repository: &RepositoryDefinition,
) -> Result<Entry, AppError> {
    let Some(default_branch) =
        git.default_branch(repository.path(), repository.default_branch())?
    else {
        return Ok(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::MissingRemoteDefaultBranch },
        ));
    };

    if let Some(reason) = default_branch_block_reason(git, repository, &default_branch)? {
        return Ok(Entry::new(repository, Outcome::Blocked { reason }));
    }

    Ok(Entry::new(repository, Outcome::Planned(Plan::CheckExisting { branch: default_branch })))
}

fn default_branch_block_reason(
    git: &impl GitClient,
    repository: &RepositoryDefinition,
    default_branch: &str,
) -> Result<Option<BlockedReason>, AppError> {
    if !git.local_branch_exists(repository.path(), default_branch)? {
        return Ok(Some(BlockedReason::MissingLocalBranch { branch: default_branch.to_string() }));
    }
    if !git.remote_branch_exists(repository.path(), default_branch)? {
        return Ok(Some(BlockedReason::MissingRemoteBranch { branch: default_branch.to_string() }));
    }
    Ok(None)
}
