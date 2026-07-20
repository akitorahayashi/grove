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
                Outcome::Cloned { .. }
                    | Outcome::Updated { .. }
                    | Outcome::UpdatedButRestorationFailed { .. }
                    | Outcome::Current { .. }
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

    let mut outcomes = Vec::with_capacity(targets.len());
    let mut outcome_repositories = Vec::with_capacity(targets.len());
    let mut pending = Vec::new();
    for repository in targets {
        outcome_repositories.push(repository);
        let path = match zoxide_path(repository.path(), resolve_symlinks) {
            Ok(path) => path,
            Err(err) => {
                outcomes.push(Some(ZoxideEntry::new(
                    repository,
                    ZoxideOutcome::Failed(err.to_string()),
                )));
                continue;
            }
        };

        if registered.contains(&path) {
            outcomes.push(Some(ZoxideEntry::new(repository, ZoxideOutcome::AlreadyRegistered)));
            continue;
        }

        match zoxide.add(&path) {
            Ok(()) => {
                outcomes.push(None);
                pending.push((outcomes.len() - 1, repository, path));
            }
            Err(err) => outcomes.push(Some(ZoxideEntry::new(
                repository,
                ZoxideOutcome::Failed(failure_message(err)),
            ))),
        }
    }

    if !pending.is_empty() {
        match zoxide.entries() {
            Ok(paths) => {
                let final_paths = registered_paths(paths, resolve_symlinks);
                for (index, repository, path) in pending {
                    let outcome = if final_paths.contains(&path) {
                        ZoxideOutcome::Added
                    } else {
                        ZoxideOutcome::Failed("zoxide did not register the repository".to_string())
                    };
                    outcomes[index] = Some(ZoxideEntry::new(repository, outcome));
                }
            }
            Err(err) => {
                let message = failure_message(err);
                for (index, repository, _) in pending {
                    outcomes[index] =
                        Some(ZoxideEntry::new(repository, ZoxideOutcome::Failed(message.clone())));
                }
            }
        }
    }

    ZoxideReport::new(
        outcomes
            .into_iter()
            .zip(outcome_repositories)
            .map(|(outcome, repository)| {
                outcome.unwrap_or_else(|| {
                    ZoxideEntry::new(
                        repository,
                        ZoxideOutcome::Failed(
                            "zoxide registration produced no final classification".to_string(),
                        ),
                    )
                })
            })
            .collect(),
    )
}

fn registered_paths(paths: Vec<PathBuf>, resolve_symlinks: bool) -> HashSet<PathBuf> {
    paths.into_iter().filter_map(|path| zoxide_path(&path, resolve_symlinks).ok()).collect()
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use tempfile::TempDir;

    use super::register;
    use crate::AppError;
    use crate::app::sync::{Entry, Outcome, ZoxideOutcome};
    use crate::repositories::{RemoteUrl, RepositoryDefinition, RepositoryName};
    use crate::zoxide::ZoxideClient;

    struct FakeZoxide {
        initial: Vec<PathBuf>,
        final_entries: Vec<PathBuf>,
        query_failure: Option<usize>,
        add_failures: HashSet<PathBuf>,
        query_count: Mutex<usize>,
        adds: Mutex<Vec<PathBuf>>,
    }

    impl FakeZoxide {
        fn new(initial: Vec<PathBuf>, final_entries: Vec<PathBuf>) -> Self {
            Self {
                initial,
                final_entries,
                query_failure: None,
                add_failures: HashSet::new(),
                query_count: Mutex::new(0),
                adds: Mutex::new(Vec::new()),
            }
        }

        fn failing_query(mut self, query: usize) -> Self {
            self.query_failure = Some(query);
            self
        }
    }

    impl ZoxideClient for FakeZoxide {
        fn verify_available(&self) -> Result<(), AppError> {
            Ok(())
        }

        fn resolve_symlinks(&self) -> bool {
            false
        }

        fn entries(&self) -> Result<Vec<PathBuf>, AppError> {
            let mut count = self.query_count.lock().unwrap();
            *count += 1;
            if self.query_failure == Some(*count) {
                return Err(AppError::zoxide_command_failed("query", "query failed"));
            }
            if *count == 1 { Ok(self.initial.clone()) } else { Ok(self.final_entries.clone()) }
        }

        fn add(&self, path: &Path) -> Result<(), AppError> {
            self.adds.lock().unwrap().push(path.to_path_buf());
            if self.add_failures.contains(path) {
                Err(AppError::zoxide_command_failed("add", "add failed"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn all_existing_uses_one_query_and_no_adds() {
        let root = TempDir::new().unwrap();
        let repositories = repositories(&root, &["first", "second"]);
        let paths = repositories.iter().map(|repository| repository.path().to_path_buf()).collect();
        let zoxide = FakeZoxide::new(paths, Vec::new());

        let report = run(&zoxide, &repositories);

        assert!(
            report
                .entries()
                .iter()
                .all(|entry| { matches!(entry.outcome(), ZoxideOutcome::AlreadyRegistered) })
        );
        assert_eq!(*zoxide.query_count.lock().unwrap(), 1);
        assert!(zoxide.adds.lock().unwrap().is_empty());
    }

    #[test]
    fn new_and_mixed_entries_use_one_final_snapshot() {
        let root = TempDir::new().unwrap();
        let repositories = repositories(&root, &["existing", "new"]);
        let existing = repositories[0].path().to_path_buf();
        let new = repositories[1].path().to_path_buf();
        let zoxide = FakeZoxide::new(vec![existing], vec![new.clone()]);

        let report = run(&zoxide, &repositories);

        assert!(matches!(report.entries()[0].outcome(), ZoxideOutcome::AlreadyRegistered));
        assert!(matches!(report.entries()[1].outcome(), ZoxideOutcome::Added));
        assert_eq!(*zoxide.query_count.lock().unwrap(), 2);
        assert_eq!(*zoxide.adds.lock().unwrap(), [new]);
    }

    #[test]
    fn add_failure_remains_per_repository() {
        let root = TempDir::new().unwrap();
        let repositories = repositories(&root, &["failed", "added"]);
        let failed = repositories[0].path().to_path_buf();
        let added = repositories[1].path().to_path_buf();
        let mut zoxide = FakeZoxide::new(Vec::new(), vec![added]);
        zoxide.add_failures.insert(failed);

        let report = run(&zoxide, &repositories);

        assert!(
            matches!(report.entries()[0].outcome(), ZoxideOutcome::Failed(message) if message == "add failed")
        );
        assert!(matches!(report.entries()[1].outcome(), ZoxideOutcome::Added));
    }

    #[test]
    fn exclusion_and_query_failures_are_explicit() {
        let root = TempDir::new().unwrap();
        let repositories = repositories(&root, &["repo"]);

        let excluded = run(&FakeZoxide::new(Vec::new(), Vec::new()), &repositories);
        assert!(matches!(
            excluded.entries()[0].outcome(),
            ZoxideOutcome::Failed(message) if message.contains("did not register")
        ));

        let initial = FakeZoxide::new(Vec::new(), Vec::new()).failing_query(1);
        let report = run(&initial, &repositories);
        assert!(
            matches!(report.entries()[0].outcome(), ZoxideOutcome::Failed(message) if message == "query failed")
        );
        assert!(initial.adds.lock().unwrap().is_empty());

        let final_query = FakeZoxide::new(Vec::new(), Vec::new()).failing_query(2);
        let report = run(&final_query, &repositories);
        assert!(
            matches!(report.entries()[0].outcome(), ZoxideOutcome::Failed(message) if message == "query failed")
        );
        assert_eq!(final_query.adds.lock().unwrap().len(), 1);
    }

    fn repositories(root: &TempDir, names: &[&str]) -> Vec<RepositoryDefinition> {
        let root_path = root.path().canonicalize().unwrap();
        names
            .iter()
            .map(|name| {
                let path = root_path.join(name);
                std::fs::create_dir(&path).unwrap();
                RepositoryDefinition::new(
                    RepositoryName::new(name).unwrap(),
                    path,
                    (*name).to_string(),
                    RemoteUrl::new(&format!("git@example.com:{name}.git")).unwrap(),
                    None,
                    root_path.join("grove.toml"),
                    root_path.clone(),
                )
            })
            .collect()
    }

    fn run(
        zoxide: &FakeZoxide,
        repositories: &[RepositoryDefinition],
    ) -> crate::app::sync::ZoxideReport {
        let references = repositories.iter().collect::<Vec<_>>();
        let entries = repositories
            .iter()
            .map(|repository| {
                Entry::new(repository, Outcome::Current { branch: "main".to_string() })
            })
            .collect::<Vec<_>>();
        register(zoxide, &references, &entries)
    }
}
