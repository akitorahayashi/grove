use std::time::Duration;

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
    pub(super) fn new(repository: &RepositoryDefinition, outcome: Outcome) -> Self {
        Self { repository: repository.display_path().to_string(), outcome }
    }

    pub fn repository(&self) -> &str {
        &self.repository
    }

    pub fn outcome(&self) -> &Outcome {
        &self.outcome
    }
}

#[derive(Debug, Clone, Copy, Default)]
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

#[derive(Debug, Clone, Copy, Default)]
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

#[derive(Debug, Clone)]
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

    pub fn planned_clones(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.outcome, Outcome::Planned(Plan::Clone { .. })))
            .count()
    }

    pub fn planned_fetches(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.outcome, Outcome::Planned(Plan::Fetch { .. })))
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
