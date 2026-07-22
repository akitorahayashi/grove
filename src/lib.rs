//! Library entry point for the grove CLI.

mod app;
mod cache;
mod cli;
mod config;
mod error;
mod git;
mod inspection;
mod phases;
mod repositories;
mod zoxide;

pub use app::api::{clone, refresh, status, sync, validate};
pub use app::clone::{Phase as ClonePhase, Report as CloneReport};
pub use app::entry::BlockedReasonDetails;
pub use app::refresh::{
    BlockedReason as RefreshBlockedReason, Entry as RefreshEntry, Outcome as RefreshOutcome,
    PhaseSummaries as RefreshPhaseSummaries, PhaseSummary as RefreshPhaseSummary,
    Plan as RefreshPlan, RefreshOptions, Report as RefreshReport,
    SkippedReason as RefreshSkippedReason,
};
pub use app::status::{
    BranchTrackingStatus, DefaultBranchStatus, RemoteUrlMismatch, StatusCondition, StatusEntry,
    StatusReport,
};
pub use app::sync::{
    BlockedReason as SyncBlockedReason, Entry as SyncEntry, Outcome as SyncOutcome,
    PhaseSummaries as SyncPhaseSummaries, PhaseSummary as SyncPhaseSummary, Plan as SyncPlan,
    Report as SyncReport, SkippedReason as SyncSkippedReason, SyncOptions, ZoxideEntry,
    ZoxideOutcome, ZoxideReport,
};
pub use app::validate::Report as ValidationReport;
pub use cache::Outcome as CacheOutcome;
pub use cli::run as cli;
pub use error::AppError;
