use std::path::Path;

use crate::AppError;
use crate::app::AppContext;
use crate::config;
use crate::git::{BranchDivergence, GitClient, NoopGitProgressSink, urls_match};
use crate::repositories::{BranchTracking, RepositoryDefinition, select_repositories};

#[derive(Debug, Clone)]
pub struct StatusReport {
    entries: Vec<StatusEntry>,
}

impl StatusReport {
    pub fn new(entries: Vec<StatusEntry>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &[StatusEntry] {
        &self.entries
    }
}

#[derive(Debug, Clone)]
pub struct StatusEntry {
    name: String,
    display_path: String,
    absolute_path: String,
    url: String,
    source_config: String,
    branch: Option<String>,
    condition: StatusCondition,
    default_branch: Option<BranchTracking>,
    remote_mismatch: Option<RemoteUrlMismatch>,
}

impl StatusEntry {
    fn from_repository(
        repository: &RepositoryDefinition,
        branch: Option<String>,
        condition: StatusCondition,
        default_branch: Option<BranchTracking>,
        remote_mismatch: Option<RemoteUrlMismatch>,
    ) -> Self {
        Self {
            name: repository.name().as_str().to_string(),
            display_path: repository.display_path().to_string(),
            absolute_path: repository.path().display().to_string(),
            url: repository.url().to_string(),
            source_config: repository.source_config().display().to_string(),
            branch,
            condition,
            default_branch,
            remote_mismatch,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn display_path(&self) -> &str {
        &self.display_path
    }

    pub fn absolute_path(&self) -> &str {
        &self.absolute_path
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn source_config(&self) -> &str {
        &self.source_config
    }

    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    pub fn condition(&self) -> &StatusCondition {
        &self.condition
    }

    pub fn default_branch(&self) -> Option<&BranchTracking> {
        self.default_branch.as_ref()
    }

    pub fn remote_mismatch(&self) -> Option<&RemoteUrlMismatch> {
        self.remote_mismatch.as_ref()
    }
}

#[derive(Debug, Clone)]
pub enum StatusCondition {
    Missing,
    Invalid(String),
    Clean,
    Dirty,
    RemoteMismatch,
    FetchFailed(String),
}

impl StatusCondition {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Missing => "missing",
            Self::Invalid(_) => "invalid",
            Self::Clean => "clean",
            Self::Dirty => "dirty",
            Self::RemoteMismatch => "remote-mismatch",
            Self::FetchFailed(_) => "fetch-failed",
        }
    }

    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Invalid(message) | Self::FetchFailed(message) => Some(message),
            Self::Missing | Self::Clean | Self::Dirty | Self::RemoteMismatch => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RemoteUrlMismatch {
    actual: String,
    expected: String,
}

impl RemoteUrlMismatch {
    pub fn new(actual: String, expected: String) -> Self {
        Self { actual, expected }
    }

    pub fn actual(&self) -> &str {
        &self.actual
    }

    pub fn expected(&self) -> &str {
        &self.expected
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
    let mut entries = Vec::new();

    for repository in repositories {
        entries.push(status_for_repository(ctx.git(), repository, fetch)?);
    }

    Ok(StatusReport::new(entries))
}

fn status_for_repository(
    git: &impl GitClient,
    repository: &RepositoryDefinition,
    fetch: bool,
) -> Result<StatusEntry, AppError> {
    if !repository.path().exists() {
        return Ok(entry(repository, None, StatusCondition::Missing, None, None));
    }

    if !repository.path().is_dir() || !git.is_work_tree(repository.path())? {
        return Ok(entry(
            repository,
            None,
            StatusCondition::Invalid("destination exists but is not a Git repository".to_string()),
            None,
            None,
        ));
    }

    if fetch {
        let mut progress = NoopGitProgressSink;
        if let Err(err) = git.fetch(repository.path(), &mut progress) {
            return Ok(entry(
                repository,
                None,
                StatusCondition::FetchFailed(err.to_string()),
                None,
                None,
            ));
        }
    }

    let branch = git.current_branch(repository.path())?;
    let clean = git.working_tree_clean(repository.path())?;
    let remote_mismatch = git.remote_url(repository.path())?.and_then(|actual| {
        if urls_match(&actual, repository.url()) {
            None
        } else {
            Some(RemoteUrlMismatch::new(actual, repository.url().to_string()))
        }
    });
    let default_branch = git.default_branch(repository.path(), repository.default_branch())?;
    let default_branch = if let Some(branch) = default_branch.as_deref() {
        branch_tracking(git, repository.path(), branch)?
    } else {
        None
    };
    let condition = if remote_mismatch.is_some() {
        StatusCondition::RemoteMismatch
    } else if clean {
        StatusCondition::Clean
    } else {
        StatusCondition::Dirty
    };

    Ok(entry(repository, branch, condition, default_branch, remote_mismatch))
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

fn entry(
    repository: &RepositoryDefinition,
    branch: Option<String>,
    condition: StatusCondition,
    default_branch: Option<BranchTracking>,
    remote_mismatch: Option<RemoteUrlMismatch>,
) -> StatusEntry {
    StatusEntry::from_repository(repository, branch, condition, default_branch, remote_mismatch)
}

#[allow(dead_code)]
fn _format_divergence(divergence: BranchDivergence) -> String {
    format!("ahead {} behind {}", divergence.ahead(), divergence.behind())
}
