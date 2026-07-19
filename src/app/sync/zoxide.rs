use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use crate::AppError;
use crate::repositories::RepositoryDefinition;
use crate::zoxide::ZoxideClient;

use super::{Entry, Outcome, Plan, ZoxideEntry, ZoxideOutcome, ZoxideReport};

pub(super) fn dry_run(repositories: &[&RepositoryDefinition], entries: &[Entry]) -> ZoxideReport {
    let entries = repositories
        .iter()
        .zip(entries)
        .filter(|(_, entry)| {
            matches!(entry.outcome(), Outcome::Planned(Plan::Clone { .. } | Plan::Fetch { .. }))
        })
        .map(|(repository, _)| ZoxideEntry::new(repository, ZoxideOutcome::WouldRegister))
        .collect();
    ZoxideReport::new(entries)
}

pub(super) fn register(
    zoxide: &impl ZoxideClient,
    repositories: &[&RepositoryDefinition],
    entries: &[Entry],
) -> ZoxideReport {
    let targets = repositories
        .iter()
        .zip(entries)
        .filter(|(_, entry)| {
            matches!(
                entry.outcome(),
                Outcome::Cloned { .. } | Outcome::Updated { .. } | Outcome::Current { .. }
            )
        })
        .map(|(repository, _)| *repository)
        .collect::<Vec<_>>();

    if targets.is_empty() {
        return ZoxideReport::new(Vec::new());
    }

    if let Err(err) = zoxide.verify_available() {
        return ZoxideReport::unavailable(failure_message(err));
    }

    let resolve_symlinks = zoxide.resolve_symlinks();
    let registered = match zoxide.entries() {
        Ok(paths) => registered_paths(paths, resolve_symlinks),
        Err(err) => {
            let message = failure_message(err);
            let entries = targets
                .into_iter()
                .map(|repository| {
                    ZoxideEntry::new(repository, ZoxideOutcome::Failed(message.clone()))
                })
                .collect();
            return ZoxideReport::new(entries);
        }
    };

    let entries = targets
        .into_iter()
        .map(|repository| register_one(zoxide, &registered, repository, resolve_symlinks))
        .collect();
    ZoxideReport::new(entries)
}

fn registered_paths(paths: Vec<PathBuf>, resolve_symlinks: bool) -> HashSet<PathBuf> {
    paths.into_iter().filter_map(|path| zoxide_path(&path, resolve_symlinks).ok()).collect()
}

fn register_one(
    zoxide: &impl ZoxideClient,
    registered: &HashSet<PathBuf>,
    repository: &RepositoryDefinition,
    resolve_symlinks: bool,
) -> ZoxideEntry {
    let path = match zoxide_path(repository.path(), resolve_symlinks) {
        Ok(path) => path,
        Err(err) => {
            return ZoxideEntry::new(repository, ZoxideOutcome::Failed(err.to_string()));
        }
    };

    if registered.contains(&path) {
        return ZoxideEntry::new(repository, ZoxideOutcome::AlreadyRegistered);
    }

    if let Err(err) = zoxide.add(&path) {
        return ZoxideEntry::new(repository, ZoxideOutcome::Failed(failure_message(err)));
    }

    match zoxide.entries() {
        Ok(paths) => {
            if registered_paths(paths, resolve_symlinks).contains(&path) {
                ZoxideEntry::new(repository, ZoxideOutcome::Added)
            } else {
                ZoxideEntry::new(
                    repository,
                    ZoxideOutcome::Failed("zoxide did not register the repository".to_string()),
                )
            }
        }
        Err(err) => ZoxideEntry::new(repository, ZoxideOutcome::Failed(failure_message(err))),
    }
}

fn zoxide_path(path: &Path, resolve_symlinks: bool) -> std::io::Result<PathBuf> {
    if resolve_symlinks { path.canonicalize() } else { Ok(normalize_absolute(path)) }
}

fn normalize_absolute(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(std::path::MAIN_SEPARATOR.to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn failure_message(err: AppError) -> String {
    match err {
        AppError::ZoxideCommandFailed { message, .. } | AppError::ZoxideUnavailable(message) => {
            message
        }
        other => other.to_string(),
    }
}
