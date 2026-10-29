//! Removed declarations: the assessment status previews, the convergence sync
//! and retire act through, and the diagnostics they report.
//!
//! Absence from the configuration never implies removal; only an explicit
//! declaration reaches this module, and even then a directory grove cannot
//! prove it owns, or whose loss would take local-only work with it, is left
//! in place.

mod assessment;
mod convergence;

use std::collections::HashSet;

use crate::config::ResolvedConfig;
use crate::repositories::{RemovedRepository, RepositoryDefinition};

pub(crate) use assessment::{Assessment, assess};
pub use assessment::{BlockedReason, LocalState};
pub use convergence::{CacheChange, Entry, Outcome, Plan};
pub(crate) use convergence::{converge, drop_cache_entry, run_phase};

/// The removed declarations a command covers: all of them, or with explicit
/// targets only those moving to a selected repository, so `gv sync <repo>`
/// never touches anything unrelated to `<repo>`.
pub(crate) fn select<'a>(
    config: &'a ResolvedConfig,
    selected: &[&RepositoryDefinition],
    targeted: bool,
) -> Vec<&'a RemovedRepository> {
    let names = selected.iter().map(|repository| repository.name()).collect::<HashSet<_>>();
    config
        .removed()
        .iter()
        .filter(|removed| !targeted || removed.moved_to().is_some_and(|to| names.contains(to)))
        .collect()
}

/// The live repository a removed declaration moves to.
pub(crate) fn destination<'a>(
    config: &'a ResolvedConfig,
    removed: &RemovedRepository,
) -> Option<&'a RepositoryDefinition> {
    let target = removed.moved_to()?;
    config.repositories().iter().find(|repository| repository.name() == target)
}
