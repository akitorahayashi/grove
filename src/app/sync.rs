use std::path::Path;

use crate::AppError;
use crate::app::AppContext;
use crate::config;
use crate::git::{GitClient, urls_match};
use crate::repositories::{RepositoryDefinition, select_repositories};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncState {
    Planned,
    Cloned,
    Updated,
    Current,
    Skipped,
    Blocked,
}

impl SyncState {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Planned => "PLANNED",
            Self::Cloned => "CLONED",
            Self::Updated => "UPDATED",
            Self::Current => "CURRENT",
            Self::Skipped => "SKIPPED",
            Self::Blocked => "BLOCKED",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyncRow {
    state: SyncState,
    repository: String,
    detail: String,
}

impl SyncRow {
    fn new(state: SyncState, repository: &RepositoryDefinition, detail: impl Into<String>) -> Self {
        Self { state, repository: repository.display_path().to_string(), detail: detail.into() }
    }

    pub fn state(&self) -> &SyncState {
        &self.state
    }

    pub fn repository(&self) -> &str {
        &self.repository
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

#[derive(Debug, Clone)]
pub struct SyncReport {
    rows: Vec<SyncRow>,
}

impl SyncReport {
    pub fn new(rows: Vec<SyncRow>) -> Self {
        Self { rows }
    }

    pub fn rows(&self) -> &[SyncRow] {
        &self.rows
    }

    pub fn count(&self, state: SyncState) -> usize {
        self.rows.iter().filter(|row| row.state == state).count()
    }
}

pub fn execute(
    ctx: &AppContext<impl GitClient>,
    config_path: Option<&Path>,
    targets: &[String],
    dry_run: bool,
) -> Result<SyncReport, AppError> {
    ctx.git().verify_available()?;
    let config = config::load(config_path)?;
    let repositories = select_repositories(config.repositories(), targets)?;
    let rows = repositories
        .into_iter()
        .map(|repository| sync_repository(ctx.git(), repository, dry_run))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(SyncReport::new(rows))
}

fn sync_repository(
    git: &impl GitClient,
    repository: &RepositoryDefinition,
    dry_run: bool,
) -> Result<SyncRow, AppError> {
    if !repository.path().exists() {
        if dry_run {
            return Ok(SyncRow::new(
                SyncState::Planned,
                repository,
                format!("clone {}", repository.url()),
            ));
        }

        return match git.clone_repository(repository.url(), repository.path()) {
            Ok(()) => Ok(SyncRow::new(SyncState::Cloned, repository, repository.url())),
            Err(err) => Ok(SyncRow::new(SyncState::Blocked, repository, err.to_string())),
        };
    }

    if !repository.path().is_dir() || !git.is_work_tree(repository.path())? {
        return Ok(SyncRow::new(
            SyncState::Blocked,
            repository,
            "destination exists but is not a Git repository",
        ));
    }

    let Some(actual_url) = git.remote_url(repository.path())? else {
        return Ok(SyncRow::new(SyncState::Blocked, repository, "remote origin is missing"));
    };
    if !urls_match(&actual_url, repository.url()) {
        return Ok(SyncRow::new(
            SyncState::Blocked,
            repository,
            "remote URL does not match grove.toml",
        ));
    }

    let Some(current_branch) = git.current_branch(repository.path())? else {
        return Ok(SyncRow::new(
            SyncState::Blocked,
            repository,
            "detached HEAD cannot be restored safely",
        ));
    };

    if !git.working_tree_clean(repository.path())? {
        return Ok(SyncRow::new(SyncState::Skipped, repository, "dirty working tree"));
    }

    if dry_run {
        return plan_existing_repository(git, repository);
    }

    if let Err(err) = git.fetch(repository.path()) {
        return Ok(SyncRow::new(SyncState::Blocked, repository, err.to_string()));
    }

    let Some(default_branch) =
        git.default_branch(repository.path(), repository.default_branch())?
    else {
        return Ok(SyncRow::new(
            SyncState::Blocked,
            repository,
            "remote default branch cannot be determined",
        ));
    };

    if let Some(reason) = default_branch_block_reason(git, repository, &default_branch)? {
        return Ok(SyncRow::new(SyncState::Blocked, repository, reason));
    }

    let Some(divergence) = git.branch_divergence(repository.path(), &default_branch)? else {
        return Ok(SyncRow::new(
            SyncState::Blocked,
            repository,
            "default branch cannot be compared with origin",
        ));
    };
    if divergence.ahead() > 0 && divergence.behind() > 0 {
        return Ok(SyncRow::new(
            SyncState::Blocked,
            repository,
            format!("{default_branch} has diverged"),
        ));
    }
    if divergence.ahead() > 0 {
        return Ok(SyncRow::new(
            SyncState::Blocked,
            repository,
            format!("{default_branch} is ahead of origin/{default_branch}"),
        ));
    }

    let update =
        match git.update_default_branch(repository.path(), &default_branch, &current_branch) {
            Ok(update) => update,
            Err(err) => return Ok(SyncRow::new(SyncState::Blocked, repository, err.to_string())),
        };

    if update.changed() {
        Ok(SyncRow::new(
            SyncState::Updated,
            repository,
            format!("{} {} -> {}", default_branch, update.before(), update.after()),
        ))
    } else {
        Ok(SyncRow::new(SyncState::Current, repository, default_branch))
    }
}

fn plan_existing_repository(
    git: &impl GitClient,
    repository: &RepositoryDefinition,
) -> Result<SyncRow, AppError> {
    let Some(default_branch) =
        git.default_branch(repository.path(), repository.default_branch())?
    else {
        return Ok(SyncRow::new(
            SyncState::Blocked,
            repository,
            "remote default branch cannot be determined",
        ));
    };

    if let Some(reason) = default_branch_block_reason(git, repository, &default_branch)? {
        return Ok(SyncRow::new(SyncState::Blocked, repository, reason));
    }

    Ok(SyncRow::new(SyncState::Planned, repository, format!("update {default_branch}")))
}

fn default_branch_block_reason(
    git: &impl GitClient,
    repository: &RepositoryDefinition,
    default_branch: &str,
) -> Result<Option<String>, AppError> {
    if !git.local_branch_exists(repository.path(), default_branch)? {
        return Ok(Some(format!("local default branch '{default_branch}' is missing")));
    }
    if !git.remote_branch_exists(repository.path(), default_branch)? {
        return Ok(Some(format!("remote default branch 'origin/{default_branch}' is missing")));
    }
    Ok(None)
}
