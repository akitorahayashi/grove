//! The `gv cache` use case: enumerating and removing local clone cache
//! entries. The cache store itself is the `crate::cache` domain; this module
//! owns only the command-level orchestration and its report.

use std::path::Path;

use crate::AppError;
use crate::cache::{EntryInfo, Store};
use crate::config;
use crate::repositories::select_repositories;

/// The result of `gv cache clean`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CleanReport {
    removed: usize,
    absent: Vec<String>,
}

impl CleanReport {
    pub(crate) fn new(removed: usize, absent: Vec<String>) -> Self {
        Self { removed, absent }
    }

    pub(crate) fn removed(&self) -> usize {
        self.removed
    }

    /// Selected repositories that had no cache entry to remove.
    pub(crate) fn absent(&self) -> &[String] {
        &self.absent
    }
}

pub(crate) fn list(store: &Store) -> Result<Vec<EntryInfo>, AppError> {
    store.list()
}

/// Remove cache entries. With no targets, every entry is removed; otherwise
/// only the selected repositories' entries are, and repositories without an
/// entry are reported as absent rather than treated as failures.
pub(crate) fn clean(
    store: &Store,
    config_path: Option<&Path>,
    targets: &[String],
) -> Result<CleanReport, AppError> {
    if targets.is_empty() {
        return Ok(CleanReport::new(store.clean_all()?, Vec::new()));
    }

    let config = config::load(config_path)?;
    let repositories = select_repositories(config.repositories(), targets)?;
    let mut removed = 0;
    let mut absent = Vec::new();
    for repository in repositories {
        if store.remove(repository.url())? {
            removed += 1;
        } else {
            absent.push(repository.display_path().to_string());
        }
    }
    Ok(CleanReport::new(removed, absent))
}
