use std::time::Duration;

use crate::app::events::PhaseSummary;
use crate::app::inspection;
use crate::app::report::Entry;

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
    LinkedWorktreeDefaultBranchConflict { branch: String },
    UpdateFailed(String),
}

impl BlockedReason {
    pub fn message(&self) -> String {
        match self {
            Self::MissingRepository => "repository is missing; run gv sync to clone it".to_string(),
            Self::DestinationNotGitRepository => {
                inspection::destination_not_git_repository().to_string()
            }
            Self::MissingOrigin => inspection::missing_origin().to_string(),
            Self::RemoteUrlMismatch => inspection::remote_url_mismatch().to_string(),
            Self::DetachedHead => "detached HEAD cannot be refreshed safely".to_string(),
            Self::FetchFailed(message) | Self::UpdateFailed(message) => message.clone(),
            Self::MissingRemoteDefaultBranch => {
                inspection::missing_remote_default_branch().to_string()
            }
            Self::MissingLocalBranch { branch } => inspection::missing_local_branch(branch),
            Self::MissingRemoteBranch { branch } => inspection::missing_remote_branch(branch),
            Self::Diverged { branch } => inspection::diverged(branch),
            Self::AheadOfOrigin { branch } => inspection::ahead_of_origin(branch),
            Self::LinkedWorktreeDefaultBranchConflict { branch } => {
                format!("multiple selected linked worktrees cannot all stay on '{branch}'")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Planned(Plan),
    Refreshed { branch: String, before: String, after: String, previous_branch: Option<String> },
    Switched { branch: String, previous_branch: String },
    SwitchedAndBlocked { branch: String, previous_branch: String, reason: BlockedReason },
    Current { branch: String },
    Skipped { reason: SkippedReason },
    Blocked { reason: BlockedReason },
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
    entries: Vec<Entry<Outcome>>,
    elapsed: Duration,
    phases: PhaseSummaries,
}

impl Report {
    pub(super) fn new(
        entries: Vec<Entry<Outcome>>,
        elapsed: Duration,
        phases: PhaseSummaries,
    ) -> Self {
        Self { entries, elapsed, phases }
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

    pub fn total(&self) -> usize {
        self.entries.len()
    }

    pub fn planned(&self) -> usize {
        self.entries.iter().filter(|entry| matches!(entry.outcome(), Outcome::Planned(_))).count()
    }

    pub fn refreshed(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry.outcome(),
                    Outcome::Refreshed { .. }
                        | Outcome::Switched { .. }
                        | Outcome::SwitchedAndBlocked { .. }
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
            .filter(|entry| {
                matches!(
                    entry.outcome(),
                    Outcome::Blocked { .. } | Outcome::SwitchedAndBlocked { .. }
                )
            })
            .count()
    }

    pub fn has_failures(&self) -> bool {
        self.entries.iter().any(|entry| {
            matches!(
                entry.outcome(),
                Outcome::Skipped { .. }
                    | Outcome::Blocked { .. }
                    | Outcome::SwitchedAndBlocked { .. }
            )
        })
    }
}
