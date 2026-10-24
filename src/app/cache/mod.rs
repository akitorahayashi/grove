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

/// Whether a `gv cache clean` target names a cache identity rather than a
/// configured repository. Repository names cannot contain `/`, and every
/// remote identity has a host and a path, so the two never overlap.
pub(crate) fn is_identity(target: &str) -> bool {
    target.contains('/')
}

/// Remove cache entries. With no targets, every entry is removed. Otherwise
/// each identity target removes its entry, and must have one: an identity is
/// typed by hand from `gv cache list`, so a miss is more likely a typo than a
/// clean cache, and it fails before anything is removed. Repository targets
/// remove their configured URL's entry, and repositories without one are
/// reported as absent rather than treated as failures. Configuration is read
/// only when a repository target needs it.
pub(crate) fn clean(
    store: &Store,
    config_path: Option<&Path>,
    targets: &[String],
) -> Result<CleanReport, AppError> {
    if targets.is_empty() {
        return Ok(CleanReport::new(store.clean_all()?, Vec::new()));
    }

    let (identities, names): (Vec<_>, Vec<_>) =
        targets.iter().cloned().partition(|target| is_identity(target));
    if let Some(missing) = identities.iter().find(|identity| !store.contains_identity(identity)) {
        return Err(AppError::cache_state(format!("no cache entry for '{missing}'")));
    }
    let repositories = if names.is_empty() {
        Vec::new()
    } else {
        let config = config::load(config_path)?;
        select_repositories(config.repositories(), &names)?.into_iter().cloned().collect()
    };

    let mut removed = 0;
    let mut absent = Vec::new();
    for identity in &identities {
        if store.remove_identity(identity)? {
            removed += 1;
        } else {
            absent.push(identity.clone());
        }
    }
    for repository in &repositories {
        if store.remove(repository.url())? {
            removed += 1;
        } else {
            absent.push(repository.display_path().to_string());
        }
    }
    Ok(CleanReport::new(removed, absent))
}
