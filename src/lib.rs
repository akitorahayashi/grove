//! Library entry point for the grove CLI.

mod app;
mod cli;
mod config;
mod error;
mod git;
mod repositories;
mod zoxide;

pub use app::api::{status, sync, validate};
pub use app::status::{
    BranchTrackingStatus, DefaultBranchStatus, RemoteUrlMismatch, StatusCondition, StatusEntry,
    StatusReport,
};
pub use app::sync::{
    BlockedReason as SyncBlockedReason, Entry as SyncEntry, Outcome as SyncOutcome,
    PhaseSummaries as SyncPhaseSummaries, PhaseSummary as SyncPhaseSummary, Plan as SyncPlan,
    Report as SyncReport, SkippedReason as SyncSkippedReason, ZoxideEntry, ZoxideOutcome,
    ZoxideReport,
};
pub use app::validate::Report as ValidationReport;
pub use cli::run as cli;
pub use error::AppError;
