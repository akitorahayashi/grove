use std::time::Duration;

use crate::repositories::RepositoryDefinition;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    branch: String,
}

impl Plan {
    pub(super) fn new(branch: String) -> Self {
        Self { branch }
    }

    pub fn branch(&self) -> &str {
        &self.branch
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
    MissingRepository,
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
}

impl BlockedReason {
    pub fn message(&self) -> String {
        match self {
            Self::MissingRepository => "repository is missing; run gv sync to clone it".to_string(),
            Self::DestinationNotGitRepository => {
                "destination exists but is not a Git worktree".to_string()
            }
            Self::MissingOrigin => "remote origin is missing".to_string(),
            Self::RemoteUrlMismatch => "remote URL does not match grove.toml".to_string(),
            Self::DetachedHead => "detached HEAD cannot be refreshed safely".to_string(),
            Self::FetchFailed(message) | Self::UpdateFailed(message) => message.clone(),
            Self::MissingRemoteDefaultBranch => {
                "remote default branch cannot be determined".to_string()
            }
            Self::MissingLocalBranch { branch } => {
                format!("local default branch '{branch}' is missing")
            }
            Self::MissingRemoteBranch { branch } => {
                format!("remote default branch 'origin/{branch}' is missing")
            }
            Self::Diverged { branch } => {
                format!("{branch} has diverged from origin/{branch}")
            }
            Self::AheadOfOrigin { branch } => {
                format!("{branch} is ahead of origin/{branch}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Planned(Plan),
    Refreshed { branch: String, before: String, after: String, previous_branch: Option<String> },
    Switched { branch: String, previous_branch: String },
    Current { branch: String },
    Skipped { reason: SkippedReason },
    Blocked { reason: BlockedReason },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    repository: String,
    outcome: Outcome,
    blocked_details: Option<BlockedReasonDetails>,
}

impl Entry {
    pub(super) fn new(repository: &RepositoryDefinition, outcome: Outcome) -> Self {
        Self { repository: repository.display_path().to_string(), outcome, blocked_details: None }
    }

    pub(super) fn blocked_with_details(
        repository: &RepositoryDefinition,
        reason: BlockedReason,
        details: BlockedReasonDetails,
    ) -> Self {
        Self {
            repository: repository.display_path().to_string(),
            outcome: Outcome::Blocked { reason },
            blocked_details: Some(details),
        }
    }

    pub fn repository(&self) -> &str {
        &self.repository
    }

    pub fn outcome(&self) -> &Outcome {
        &self.outcome
    }

    pub(crate) fn blocked_details(&self) -> Option<&BlockedReasonDetails> {
        self.blocked_details.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BlockedReasonDetails {
    RemoteUrlMismatch { actual: String, expected: String },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhaseSummary {
    count: usize,
    elapsed: Duration,
}

impl PhaseSummary {
    pub(super) fn new(count: usize, elapsed: Duration) -> Self {
        Self { count, elapsed }
    }

    pub fn count(self) -> usize {
        self.count
    }

    pub fn elapsed(self) -> Duration {
        self.elapsed
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhaseSummaries {
    checked: PhaseSummary,
    fetched: PhaseSummary,
    refreshed: PhaseSummary,
}

impl PhaseSummaries {
    pub(super) fn new(
        checked: PhaseSummary,
        fetched: PhaseSummary,
        refreshed: PhaseSummary,
    ) -> Self {
        Self { checked, fetched, refreshed }
    }

    pub fn checked(self) -> PhaseSummary {
        self.checked
    }

    pub fn fetched(self) -> PhaseSummary {
        self.fetched
    }

    pub fn refreshed(self) -> PhaseSummary {
        self.refreshed
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    entries: Vec<Entry>,
    elapsed: Duration,
    phases: PhaseSummaries,
}

impl Report {
    pub(super) fn new(entries: Vec<Entry>, elapsed: Duration, phases: PhaseSummaries) -> Self {
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

    pub fn planned(&self) -> usize {
        self.entries.iter().filter(|entry| matches!(entry.outcome, Outcome::Planned(_))).count()
    }

    pub fn refreshed(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| {
                matches!(entry.outcome, Outcome::Refreshed { .. } | Outcome::Switched { .. })
            })
            .count()
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
