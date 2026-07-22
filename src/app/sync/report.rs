use std::time::Duration;

use crate::app::cache::CacheOutcome;
use crate::app::entry::Entry;
use crate::inspection;
use crate::phases::Summary as PhaseSummary;
use crate::repositories::RepositoryDefinition;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Clone { url: String },
    Fetch { branch: String },
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
    Diverged { branch: String },
    AheadOfOrigin { branch: String },
    UpdateFailed(String),
    CloneFailed(String),
}

impl BlockedReason {
    pub fn message(&self) -> String {
        match self {
            Self::DestinationNotGitRepository => {
                inspection::destination_not_git_repository().to_string()
            }
            Self::MissingOrigin => inspection::missing_origin().to_string(),
            Self::RemoteUrlMismatch => inspection::remote_url_mismatch().to_string(),
            Self::DetachedHead => "detached HEAD cannot be restored safely".to_string(),
            Self::FetchFailed(message) => message.clone(),
            Self::MissingRemoteDefaultBranch => {
                inspection::missing_remote_default_branch().to_string()
            }
            Self::MissingLocalBranch { branch } => inspection::missing_local_branch(branch),
            Self::MissingRemoteBranch { branch } => inspection::missing_remote_branch(branch),
            Self::Diverged { branch } => inspection::diverged(branch),
            Self::AheadOfOrigin { branch } => inspection::ahead_of_origin(branch),
            Self::UpdateFailed(message) | Self::CloneFailed(message) => message.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Planned(Plan),
    Cloned { url: String, cache: CacheOutcome },
    Updated { branch: String, before: String, after: String },
    UpdatedButRestorationFailed { branch: String, before: String, after: String, message: String },
    Current { branch: String },
    Skipped { reason: SkippedReason },
    Blocked { reason: BlockedReason },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhaseSummaries {
    checked: PhaseSummary,
    prepared: PhaseSummary,
    updated: PhaseSummary,
}

impl PhaseSummaries {
    pub(super) fn new(
        checked: PhaseSummary,
        prepared: PhaseSummary,
        updated: PhaseSummary,
    ) -> Self {
        Self { checked, prepared, updated }
    }

    pub fn checked(self) -> PhaseSummary {
        self.checked
    }

    pub fn prepared(self) -> PhaseSummary {
        self.prepared
    }

    pub fn updated(self) -> PhaseSummary {
        self.updated
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    entries: Vec<Entry<Outcome>>,
    elapsed: Duration,
    phases: PhaseSummaries,
    zoxide: Option<ZoxideReport>,
}

impl Report {
    pub(super) fn new(
        entries: Vec<Entry<Outcome>>,
        elapsed: Duration,
        phases: PhaseSummaries,
        zoxide: Option<ZoxideReport>,
    ) -> Self {
        Self { entries, elapsed, phases, zoxide }
    }

    pub fn entries(&self) -> &[Entry<Outcome>] {
        &self.entries
    }

    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    pub fn phases(&self) -> PhaseSummaries {
        self.phases
    }

    pub fn zoxide(&self) -> Option<&ZoxideReport> {
        self.zoxide.as_ref()
    }

    pub fn total(&self) -> usize {
        self.entries.len()
    }

    pub fn planned_clones(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.outcome(), Outcome::Planned(Plan::Clone { .. })))
            .count()
    }

    pub fn planned_fetches(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.outcome(), Outcome::Planned(Plan::Fetch { .. })))
            .count()
    }

    pub fn cloned(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.outcome(), Outcome::Cloned { .. }))
            .count()
    }

    pub fn updated(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry.outcome(),
                    Outcome::Updated { .. } | Outcome::UpdatedButRestorationFailed { .. }
                )
            })
            .count()
    }

    pub fn skipped(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.outcome(), Outcome::Skipped { .. }))
            .count()
    }

    pub fn blocked(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.outcome(), Outcome::Blocked { .. }))
            .count()
    }

    pub fn has_failures(&self) -> bool {
        self.entries.iter().any(|entry| {
            matches!(
                entry.outcome(),
                Outcome::Skipped { .. }
                    | Outcome::Blocked { .. }
                    | Outcome::UpdatedButRestorationFailed { .. }
            )
        }) || self.zoxide.as_ref().is_some_and(ZoxideReport::has_failures)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoxideOutcome {
    WouldRegister,
    Added,
    AlreadyRegistered,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoxideEntry {
    repository: String,
    outcome: ZoxideOutcome,
}

impl ZoxideEntry {
    pub(super) fn new(repository: &RepositoryDefinition, outcome: ZoxideOutcome) -> Self {
        Self { repository: repository.display_path().to_string(), outcome }
    }

    pub fn repository(&self) -> &str {
        &self.repository
    }

    pub fn outcome(&self) -> &ZoxideOutcome {
        &self.outcome
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoxideReport {
    entries: Vec<ZoxideEntry>,
    unavailable: Option<String>,
}

impl ZoxideReport {
    pub(super) fn new(entries: Vec<ZoxideEntry>) -> Self {
        Self { entries, unavailable: None }
    }

    pub(super) fn unavailable(message: String) -> Self {
        Self { entries: Vec::new(), unavailable: Some(message) }
    }

    pub fn entries(&self) -> &[ZoxideEntry] {
        &self.entries
    }

    pub fn unavailable_message(&self) -> Option<&str> {
        self.unavailable.as_deref()
    }

    pub fn has_failures(&self) -> bool {
        self.unavailable.is_some()
            || self.entries.iter().any(|entry| matches!(entry.outcome(), ZoxideOutcome::Failed(_)))
    }
}
