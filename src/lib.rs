//! Library entry point for the grove CLI.

mod app;
mod cli;
mod config;
mod error;
mod git;
mod repositories;
mod zoxide;

pub use app::api::{refresh, status, sync, validate};
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
pub use cli::run as cli;
pub use error::AppError;
