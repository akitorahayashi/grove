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
    let parallelism = std::thread::available_parallelism()?.get();

    Ok(StatusReport::new(collect_entries(ctx.git(), &repositories, fetch, parallelism)?))
}

/// Collect one status entry per repository, preserving selection order. The
/// no-fetch path uses bounded parallel workers; `--fetch` additionally keys
/// workers by Git common directory, matching sync and refresh, so linked
/// worktrees sharing a common directory are serialized.
fn collect_entries(
    git: &impl RepositoryProbe,
    repositories: &[&RepositoryDefinition],
    fetch: bool,
    parallelism: usize,
) -> Result<Vec<StatusEntry>, AppError> {
    if !fetch {
        let results = workers::map(repositories, parallelism, |repository| {
            status_for_repository(git, repository)
        })?;
        return results.into_iter().collect();
    }

    let preflights =
        workers::map(repositories, parallelism, |repository| fetch_preflight(git, repository))?;
    let mut entries = std::iter::repeat_with(|| None).take(repositories.len()).collect::<Vec<_>>();
    let mut tasks = Vec::new();
    for (index, preflight) in preflights.into_iter().enumerate() {
        match preflight? {
            FetchPreflight::Entry(entry) => entries[index] = Some(*entry),
            FetchPreflight::Task { common_directory } => {
                tasks.push(FetchTask { index, repository: repositories[index], common_directory });
            }
        }
    }

    let fetched = workers::map_keyed(
        &tasks,
        parallelism,
        |task| task.common_directory.clone(),
        |task| fetch_status(git, task),
    )?;
    for result in fetched {
        let (index, entry) = result?;
        entries[index] = Some(entry);
    }
    entries
        .into_iter()
        .map(|entry| entry.ok_or_else(|| AppError::internal("status preflight omitted an entry")))
        .collect()
}

struct FetchTask<'a> {
    index: usize,
    repository: &'a RepositoryDefinition,
    common_directory: PathBuf,
}

enum FetchPreflight {
    Entry(Box<StatusEntry>),
    Task { common_directory: PathBuf },
}

fn fetch_preflight(
    git: &impl RepositoryProbe,
    repository: &RepositoryDefinition,
) -> Result<FetchPreflight, AppError> {
    if !repository.path().exists() {
        return Ok(FetchPreflight::Entry(Box::new(StatusEntry::from_repository(
            repository,
            None,
            StatusCondition::Missing,
            None,
            None,
        ))));
    }

    if !repository.path().is_dir() || !git.is_work_tree(repository.path())? {
        return Ok(FetchPreflight::Entry(Box::new(StatusEntry::from_repository(
            repository,
            None,
            StatusCondition::Invalid(inspection::destination_not_git_repository().to_string()),
            None,
            None,
        ))));
    }

    let Some(actual) = git.remote_url(repository.path())? else {
        return Ok(FetchPreflight::Entry(Box::new(StatusEntry::from_repository(
            repository,
            None,
            StatusCondition::Invalid(inspection::missing_origin().to_string()),
            None,
            None,
        ))));
    };
    if !urls_match(&actual, repository.url()) {
        return Ok(FetchPreflight::Entry(Box::new(status_for_repository(git, repository)?)));
    }

    Ok(FetchPreflight::Task { common_directory: git.common_directory(repository.path())? })
}

fn fetch_status(
    git: &impl RepositoryProbe,
    task: &FetchTask<'_>,
) -> Result<(usize, StatusEntry), AppError> {
    let _lock = git.lock_repository(&task.common_directory)?;
    let Some(actual) = git.remote_url(task.repository.path())? else {
        return Ok((
            task.index,
            StatusEntry::from_repository(
                task.repository,
                None,
                StatusCondition::Invalid(inspection::missing_origin().to_string()),
                None,
                None,
            ),
        ));
    };
    if !urls_match(&actual, task.repository.url()) {
        return Ok((task.index, status_for_repository(git, task.repository)?));
    }

    let mut progress = NoopGitProgressSink;
    if let Err(error) = git.fetch(task.repository.path(), &mut progress) {
        return Ok((
            task.index,
            StatusEntry::from_repository(
                task.repository,
                None,
                StatusCondition::FetchFailed(error.to_string()),
                None,
                None,
            ),
        ));
    }
    Ok((task.index, status_for_repository(git, task.repository)?))
}

fn status_for_repository(
    git: &impl RepositoryProbe,
    repository: &RepositoryDefinition,
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

    let Some(worktree) = git.worktree_status(repository.path())? else {
        return Ok(StatusEntry::from_repository(
            repository,
            None,
            StatusCondition::Invalid(inspection::destination_not_git_repository().to_string()),
            None,
            None,
        ));
    };
    let branch = worktree.branch().map(str::to_string);
    let actual = git.remote_url(repository.path())?;
    let remote_mismatch = actual.as_ref().and_then(|actual| {
        (!urls_match(actual, repository.url()))
            .then(|| RemoteUrlMismatch::new(actual.to_string(), repository.url().to_string()))
    });
    let default_branch = git.default_branch(repository.path(), repository.default_branch())?;
    let default_branch = if let Some(branch) = default_branch.as_deref() {
        default_branch_status(git, repository, branch)?
    } else {
        None
    };
    let condition = if actual.is_none() {
        StatusCondition::Invalid(inspection::missing_origin().to_string())
    } else if remote_mismatch.is_some() {
        StatusCondition::RemoteMismatch
    } else if worktree.is_clean() {
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
    use crate::git::{BranchTracking, GitProgressSink, RepositoryProbe, WorktreeStatus};
    use crate::repositories::{BranchName, RemoteUrl, RepositoryDefinition, RepositoryName};

    struct BarrierGit {
        fetch_barrier: Option<Arc<Barrier>>,
        worktree_barrier: Option<Arc<Barrier>>,
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
            if let Some(barrier) = &self.fetch_barrier {
                barrier.wait();
            }
            Ok(())
        }

        fn common_directory(&self, repository: &Path) -> Result<PathBuf, AppError> {
            Ok(repository.to_path_buf())
        }

        fn is_work_tree(&self, _repository: &Path) -> Result<bool, AppError> {
            Ok(true)
        }

        fn worktree_status(&self, _repository: &Path) -> Result<Option<WorktreeStatus>, AppError> {
            if let Some(barrier) = &self.worktree_barrier {
                barrier.wait();
            }
            Ok(Some(WorktreeStatus::new(Some("main".to_string()), true)))
        }

        fn remote_url(&self, _repository: &Path) -> Result<Option<RemoteUrl>, AppError> {
            Ok(Some(RemoteUrl::new("https://example.com/repo.git").unwrap()))
        }

        fn default_branch(
            &self,
            _repository: &Path,
            _configured: Option<&BranchName>,
        ) -> Result<Option<String>, AppError> {
            Ok(Some("main".to_string()))
        }

        fn branch_tracking(
            &self,
            _repository: &Path,
            _branch: &BranchName,
        ) -> Result<BranchTracking, AppError> {
            Ok(BranchTracking::Divergence { ahead: 0, behind: 0 })
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
        let git = BarrierGit {
            fetch_barrier: Some(Arc::new(Barrier::new(count))),
            worktree_barrier: None,
        };
        let entries = collect_entries(&git, &repositories, true, count).unwrap();

        assert_eq!(entries.len(), count);
        assert!(entries.iter().all(|entry| matches!(entry.condition(), StatusCondition::Clean)));
    }

    #[test]
    fn status_runs_independent_repositories_concurrently() {
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
        let git = BarrierGit {
            fetch_barrier: None,
            worktree_barrier: Some(Arc::new(Barrier::new(count))),
        };

        let entries = collect_entries(&git, &repositories, false, count).unwrap();

        assert_eq!(entries.len(), count);
        assert!(entries.iter().all(|entry| matches!(entry.condition(), StatusCondition::Clean)));
    }
}
