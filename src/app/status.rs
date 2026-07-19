use std::path::Path;

use crate::AppError;
use crate::app::AppContext;
use crate::config;
use crate::git::{BranchDivergence, GitClient, urls_match};
use crate::repositories::{
    BranchTracking, RepositoryCondition, RepositoryDefinition, RepositoryState, select_repositories,
};

#[derive(Debug, Clone)]
pub struct StatusReport {
    rows: Vec<StatusRow>,
}

impl StatusReport {
    pub fn new(rows: Vec<StatusRow>) -> Self {
        Self { rows }
    }

    pub fn rows(&self) -> &[StatusRow] {
        &self.rows
    }
}

#[derive(Debug, Clone)]
pub struct StatusRow {
    repository: String,
    branch: String,
    state: String,
    default_branch: String,
}

impl StatusRow {
    pub fn new(repository: String, branch: String, state: String, default_branch: String) -> Self {
        Self { repository, branch, state, default_branch }
    }

    pub fn repository(&self) -> &str {
        &self.repository
    }

    pub fn branch(&self) -> &str {
        &self.branch
    }

    pub fn state(&self) -> &str {
        &self.state
    }

    pub fn default_branch(&self) -> &str {
        &self.default_branch
    }
}

pub fn execute(
    ctx: &AppContext<impl GitClient>,
    config_path: Option<&Path>,
    targets: &[String],
    fetch: bool,
) -> Result<StatusReport, AppError> {
    ctx.git().verify_available()?;
    let config = config::load(config_path)?;
    let repositories = select_repositories(config.repositories(), targets)?;
    let mut rows = Vec::new();

    for repository in repositories {
        rows.push(status_for_repository(ctx.git(), repository, fetch)?);
    }

    Ok(StatusReport::new(rows))
}

fn status_for_repository(
    git: &impl GitClient,
    repository: &RepositoryDefinition,
    fetch: bool,
) -> Result<StatusRow, AppError> {
    if !repository.path().exists() {
        return Ok(row(repository, RepositoryState::new(None, RepositoryCondition::Missing, None)));
    }

    if !repository.path().is_dir() || !git.is_work_tree(repository.path())? {
        return Ok(row(
            repository,
            RepositoryState::new(
                None,
                RepositoryCondition::Invalid(
                    "destination exists but is not a Git repository".to_string(),
                ),
                None,
            ),
        ));
    }

    if fetch && let Err(err) = git.fetch(repository.path()) {
        return Ok(StatusRow::new(
            repository.display_path().to_string(),
            "-".to_string(),
            format!("fetch-failed: {err}"),
            "-".to_string(),
        ));
    }

    let branch = git.current_branch(repository.path())?;
    let clean = git.working_tree_clean(repository.path())?;
    let remote_mismatch = git
        .remote_url(repository.path())?
        .is_some_and(|actual| !urls_match(&actual, repository.url()));
    let default_branch = git.default_branch(repository.path(), repository.default_branch())?;
    let default_branch = if let Some(branch) = default_branch.as_deref() {
        branch_tracking(git, repository.path(), branch)?
    } else {
        None
    };
    let condition = if remote_mismatch {
        RepositoryCondition::RemoteMismatch
    } else if clean {
        RepositoryCondition::Clean
    } else {
        RepositoryCondition::Dirty
    };

    Ok(row(repository, RepositoryState::new(branch, condition, default_branch)))
}

fn branch_tracking(
    git: &impl GitClient,
    repository: &Path,
    branch: &str,
) -> Result<Option<BranchTracking>, AppError> {
    let Some(divergence) = git.branch_divergence(repository, branch)? else {
        return Ok(Some(BranchTracking::new(branch.to_string(), 0, 0)));
    };
    Ok(Some(BranchTracking::new(branch.to_string(), divergence.ahead(), divergence.behind())))
}

fn row(repository: &RepositoryDefinition, state: RepositoryState) -> StatusRow {
    let branch = state.branch().unwrap_or("-").to_string();
    let default_branch =
        state.default_branch().map(format_tracking).unwrap_or_else(|| "-".to_string());
    StatusRow::new(
        repository.display_path().to_string(),
        branch,
        state.condition().as_str().to_string(),
        default_branch,
    )
}

fn format_tracking(tracking: &BranchTracking) -> String {
    let mut parts = vec![tracking.branch().to_string()];
    if tracking.ahead() > 0 {
        parts.push(format!("ahead {}", tracking.ahead()));
    }
    if tracking.behind() > 0 {
        parts.push(format!("behind {}", tracking.behind()));
    }
    parts.join(" ")
}

#[allow(dead_code)]
fn _format_divergence(divergence: BranchDivergence) -> String {
    format!("ahead {} behind {}", divergence.ahead(), divergence.behind())
}
