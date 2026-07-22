use std::path::{Path, PathBuf};

use crate::AppError;
use crate::app::AppContext;
use crate::config;
use crate::git::{GitClient, NoopGitProgressSink, RepositoryProbe, urls_match};
use crate::inspection::{self, BranchReadiness};
use crate::phases::workers;
use crate::repositories::RepositoryDefinition;
use crate::repositories::select_repositories;

#[derive(Debug, Clone)]
pub struct StatusReport {
    entries: Vec<StatusEntry>,
}

impl StatusReport {
    pub(crate) fn new(entries: Vec<StatusEntry>) -> Self {
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
    default_branch: Option<DefaultBranchStatus>,
    remote_mismatch: Option<RemoteUrlMismatch>,
}

impl StatusEntry {
    fn from_repository(
        repository: &RepositoryDefinition,
        branch: Option<String>,
        condition: StatusCondition,
        default_branch: Option<DefaultBranchStatus>,
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

    pub fn default_branch(&self) -> Option<&DefaultBranchStatus> {
        self.default_branch.as_ref()
    }

    pub fn remote_mismatch(&self) -> Option<&RemoteUrlMismatch> {
        self.remote_mismatch.as_ref()
    }
}

#[derive(Debug, Clone)]
pub struct DefaultBranchStatus {
    branch: String,
    tracking: BranchTrackingStatus,
}

impl DefaultBranchStatus {
    pub(crate) fn new(branch: String, tracking: BranchTrackingStatus) -> Self {
        Self { branch, tracking }
    }

    pub fn branch(&self) -> &str {
        &self.branch
    }

    pub fn tracking(&self) -> &BranchTrackingStatus {
        &self.tracking
    }
}

#[derive(Debug, Clone)]
pub enum BranchTrackingStatus {
    Divergence { ahead: u32, behind: u32 },
    MissingLocalBranch,
    MissingRemoteBranch,
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
    pub(crate) fn new(actual: String, expected: String) -> Self {
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
    let parallelism = if fetch { std::thread::available_parallelism()?.get() } else { 1 };

    Ok(StatusReport::new(collect_entries(ctx.git(), &repositories, fetch, parallelism)?))
}

/// Collect one status entry per repository, preserving selection order. The
/// no-fetch path stays serial; `--fetch` runs through bounded parallel workers
/// keyed by Git common directory, matching sync and refresh, so linked
/// worktrees sharing a common directory are serialized.
fn collect_entries(
    git: &impl RepositoryProbe,
    repositories: &[&RepositoryDefinition],
    fetch: bool,
    parallelism: usize,
) -> Result<Vec<StatusEntry>, AppError> {
    if !fetch {
        return repositories
            .iter()
            .map(|repository| status_for_repository(git, repository, fetch))
            .collect();
    }

    let results = workers::map_keyed(
        repositories,
        parallelism,
        |repository| status_resource(git, repository),
        |repository| status_for_repository(git, repository, fetch),
    )?;
    results.into_iter().collect()
}

// Group repositories by Git common directory so linked worktrees of one
// repository serialize their fetches. A valid work tree always resolves its
// common directory, so the probe fails only for missing or non-repository
// paths; those never fetch (`status_for_repository` reports them Missing or
// Invalid), so keying them on their own path is the harmless fallback the
// status --fetch design specifies.
fn status_resource(git: &impl RepositoryProbe, repository: &RepositoryDefinition) -> PathBuf {
    git.common_directory(repository.path()).unwrap_or_else(|_| repository.path().to_path_buf())
}

fn status_for_repository(
    git: &impl RepositoryProbe,
    repository: &RepositoryDefinition,
    fetch: bool,
) -> Result<StatusEntry, AppError> {
    if !repository.path().exists() {
        return Ok(StatusEntry::from_repository(
            repository,
            None,
            StatusCondition::Missing,
            None,
            None,
        ));
    }

    if !repository.path().is_dir() || !git.is_work_tree(repository.path())? {
        return Ok(StatusEntry::from_repository(
            repository,
            None,
            StatusCondition::Invalid(inspection::destination_not_git_repository().to_string()),
            None,
            None,
        ));
    }

    if fetch {
        let mut progress = NoopGitProgressSink;
        if let Err(err) = git.fetch(repository.path(), &mut progress) {
            return Ok(StatusEntry::from_repository(
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
            Some(RemoteUrlMismatch::new(actual.to_string(), repository.url().to_string()))
        }
    });
    let default_branch = git.default_branch(repository.path(), repository.default_branch())?;
    let default_branch = if let Some(branch) = default_branch.as_deref() {
        default_branch_status(git, repository, branch)?
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

    Ok(StatusEntry::from_repository(repository, branch, condition, default_branch, remote_mismatch))
}

fn default_branch_status(
    git: &impl RepositoryProbe,
    repository: &RepositoryDefinition,
    branch: &str,
) -> Result<Option<DefaultBranchStatus>, AppError> {
    let tracking = match inspection::branch_readiness(git, repository, branch)? {
        BranchReadiness::MissingLocal => BranchTrackingStatus::MissingLocalBranch,
        BranchReadiness::MissingRemote => BranchTrackingStatus::MissingRemoteBranch,
        BranchReadiness::Divergence { ahead, behind } => {
            BranchTrackingStatus::Divergence { ahead, behind }
        }
    };
    Ok(Some(DefaultBranchStatus::new(branch.to_string(), tracking)))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Barrier};

    use super::{AppError, StatusCondition, collect_entries};
    use crate::git::{BranchDivergence, GitProgressSink, RepositoryProbe};
    use crate::repositories::{BranchName, RemoteUrl, RepositoryDefinition, RepositoryName};

    struct BarrierGit {
        barrier: Arc<Barrier>,
    }

    impl RepositoryProbe for BarrierGit {
        fn verify_available(&self) -> Result<(), AppError> {
            Ok(())
        }

        fn fetch(
            &self,
            _repository: &Path,
            _progress: &mut dyn GitProgressSink,
        ) -> Result<(), AppError> {
            self.barrier.wait();
            Ok(())
        }

        fn common_directory(&self, repository: &Path) -> Result<PathBuf, AppError> {
            Ok(repository.to_path_buf())
        }

        fn is_work_tree(&self, _repository: &Path) -> Result<bool, AppError> {
            Ok(true)
        }

        fn current_branch(&self, _repository: &Path) -> Result<Option<String>, AppError> {
            Ok(Some("main".to_string()))
        }

        fn working_tree_clean(&self, _repository: &Path) -> Result<bool, AppError> {
            Ok(true)
        }

        fn remote_url(&self, _repository: &Path) -> Result<Option<RemoteUrl>, AppError> {
            Ok(None)
        }

        fn default_branch(
            &self,
            _repository: &Path,
            _configured: Option<&BranchName>,
        ) -> Result<Option<String>, AppError> {
            Ok(Some("main".to_string()))
        }

        fn local_branch_exists(&self, _repository: &Path, _branch: &str) -> Result<bool, AppError> {
            Ok(true)
        }

        fn remote_branch_exists(
            &self,
            _repository: &Path,
            _branch: &str,
        ) -> Result<bool, AppError> {
            Ok(true)
        }

        fn branch_divergence(
            &self,
            _repository: &Path,
            _branch: &str,
        ) -> Result<BranchDivergence, AppError> {
            Ok(BranchDivergence::new(0, 0))
        }

        fn short_revision(&self, _repository: &Path, _reference: &str) -> Result<String, AppError> {
            Ok("0000000".to_string())
        }
    }

    #[test]
    fn fetch_status_runs_independent_repositories_concurrently() {
        let root = tempfile::tempdir().unwrap();
        let count = 4;
        let definitions = (0..count)
            .map(|index| {
                let path = root.path().join(format!("repo{index}"));
                std::fs::create_dir_all(&path).unwrap();
                RepositoryDefinition::new(
                    RepositoryName::new(&format!("repo{index}")).unwrap(),
                    path,
                    format!("repo{index}"),
                    RemoteUrl::new("https://example.com/repo.git").unwrap(),
                    None,
                    root.path().join("grove.toml"),
                    root.path().to_path_buf(),
                )
            })
            .collect::<Vec<_>>();
        let repositories = definitions.iter().collect::<Vec<_>>();

        // A serial fetch would leave every fetch after the first waiting on a
        // barrier that never fills; concurrent execution lets all `count`
        // fetches arrive together and proceed. Each repository has a distinct
        // common directory, so none serialize against another.
        let git = BarrierGit { barrier: Arc::new(Barrier::new(count)) };
        let entries = collect_entries(&git, &repositories, true, count).unwrap();

        assert_eq!(entries.len(), count);
        assert!(entries.iter().all(|entry| matches!(entry.condition(), StatusCondition::Clean)));
    }
}
