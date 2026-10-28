use std::time::Duration;

use crate::app::entry::Entry;
use crate::cache::Outcome as CacheOutcome;
use crate::inspection;
use crate::phases::Summary as PhaseSummary;
use crate::removal;
use crate::repositories::RepositoryDefinition;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Plan {
    Clone {
        url: String,
    },
    Fetch {
        branch: String,
    },
    /// The destination of a planned move, which a dry run does not clone.
    Move {
        from: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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
#[non_exhaustive]
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
#[non_exhaustive]
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
    removed: PhaseSummary,
    checked: PhaseSummary,
    prepared: PhaseSummary,
    updated: PhaseSummary,
    seeded: PhaseSummary,
}

impl PhaseSummaries {
    pub(super) fn new(
        removed: PhaseSummary,
        checked: PhaseSummary,
        prepared: PhaseSummary,
        updated: PhaseSummary,
        seeded: PhaseSummary,
    ) -> Self {
        Self { removed, checked, prepared, updated, seeded }
    }

    pub fn removed(self) -> PhaseSummary {
        self.removed
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

    pub fn seeded(self) -> PhaseSummary {
        self.seeded
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    removals: Vec<removal::Entry>,
    entries: Vec<Entry<Outcome>>,
    elapsed: Duration,
    phases: PhaseSummaries,
    zoxide: Option<ZoxideReport>,
}

impl Report {
    pub(super) fn new(
        removals: Vec<removal::Entry>,
        entries: Vec<Entry<Outcome>>,
        elapsed: Duration,
        phases: PhaseSummaries,
        zoxide: Option<ZoxideReport>,
    ) -> Self {
        Self { removals, entries, elapsed, phases, zoxide }
    }

    /// The removed declarations sync acted on or would act on; an absent old
    /// path with no cache entry to drop is omitted.
    pub fn removals(&self) -> &[removal::Entry] {
        &self.removals
    }

    pub fn entries(&self) -> &[Entry<Outcome>] {
        &self.entries
    }

    pub fn planned_moves(&self) -> usize {
        self.removals
            .iter()
            .filter(|entry| {
                matches!(entry.outcome(), removal::Outcome::Planned(removal::Plan::Move { .. }))
            })
            .count()
    }

    pub fn planned_trashes(&self) -> usize {
        self.removals
            .iter()
            .filter(|entry| {
                matches!(entry.outcome(), removal::Outcome::Planned(removal::Plan::Trash))
            })
            .count()
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
            + self.blocked_removals()
    }

    fn blocked_removals(&self) -> usize {
        self.removals
            .iter()
            .filter(|entry| matches!(entry.outcome(), removal::Outcome::Blocked { .. }))
            .count()
    }

    pub fn has_failures(&self) -> bool {
        self.blocked_removals() > 0
            || self.entries.iter().any(|entry| {
                matches!(
                    entry.outcome(),
                    Outcome::Skipped { .. }
                        | Outcome::Blocked { .. }
                        | Outcome::UpdatedButRestorationFailed { .. }
                )
            })
            || self.zoxide.as_ref().is_some_and(ZoxideReport::has_failures)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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
