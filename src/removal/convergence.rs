use std::fs;
use std::time::Instant;

use crate::AppError;
use crate::cache::Store;
use crate::config::ResolvedConfig;
use crate::git::{Removal, RepositoryProbe};
use crate::phases::{Event, EventSink, Summary};
use crate::repositories::{RemovedRepository, RepositoryDefinition, RepositoryName};
use crate::trash::Trash;

use super::{Assessment, BlockedReason, assess, destination};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Plan {
    Move { to: String },
    Trash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    Absent,
    Planned(Plan),
    Moved { to: String },
    Trashed,
    Blocked { reason: BlockedReason },
}

/// What happened to the clone cache entry of a removed declaration's URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CacheChange {
    Untouched,
    WouldRemove,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    repository: String,
    outcome: Outcome,
    cache: CacheChange,
    warning: Option<String>,
    planned_destination: Option<RepositoryName>,
}

impl Entry {
    pub fn repository(&self) -> &str {
        &self.repository
    }

    pub fn outcome(&self) -> &Outcome {
        &self.outcome
    }

    pub fn cache(&self) -> CacheChange {
        self.cache
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    /// The live repository a dry run plans to fill by this move.
    pub(crate) fn planned_destination(&self) -> Option<&RepositoryName> {
        self.planned_destination.as_ref()
    }

    fn is_reportable(&self) -> bool {
        self.outcome != Outcome::Absent
            || self.cache != CacheChange::Untouched
            || self.warning.is_some()
    }
}

/// Entries run sequentially: removals are rare, and moving a directory must
/// not race the probes of another entry over the same tree.
pub(crate) fn run_phase<P: Copy>(
    removed: &[&RemovedRepository],
    phase: P,
    events: &impl EventSink<P>,
    mut converge: impl FnMut(&RemovedRepository) -> Result<Entry, AppError>,
) -> Result<(Vec<Entry>, Summary), AppError> {
    if removed.is_empty() {
        return Ok((Vec::new(), Summary::default()));
    }

    events.emit(Event::PhaseStarted { phase, total: removed.len() })?;
    let started = Instant::now();
    let mut entries = Vec::new();
    for removed in removed {
        let repository = removed.display_path().to_string();
        events.emit(Event::RepositoryStarted { repository: repository.clone(), phase })?;
        let entry = match converge(removed) {
            Ok(entry) => entry,
            Err(error) => {
                events.emit(Event::PhaseFailed { phase })?;
                return Err(error);
            }
        };
        events.emit(Event::RepositoryFinished { repository, phase })?;
        entries.push(entry);
    }

    let changed = entries
        .iter()
        .filter(|entry| matches!(entry.outcome, Outcome::Moved { .. } | Outcome::Trashed))
        .count();
    let summary = Summary::new(changed, started.elapsed());
    events.emit(Event::PhaseCompleted { phase, summary })?;
    entries.retain(Entry::is_reportable);
    Ok((entries, summary))
}

/// `config` is the configuration that stays in force: destinations resolve
/// against its repositories, and a cache entry one of them shares is kept.
pub(crate) fn converge<'s>(
    git: &(impl RepositoryProbe + Removal),
    trash: &impl Trash,
    cache: &dyn Fn() -> Result<&'s Store, AppError>,
    config: &ResolvedConfig,
    removed: &RemovedRepository,
    dry_run: bool,
) -> Result<Entry, AppError> {
    let destination = destination(config, removed);
    let assessment = assess(git, removed, destination)?;
    let (outcome, warning) = match assessment {
        Assessment::Absent => (Outcome::Absent, None),
        Assessment::Blocked(reason) => (Outcome::Blocked { reason }, None),
        Assessment::Move { to } if dry_run => (Outcome::Planned(Plan::Move { to }), None),
        Assessment::Trash if dry_run => (Outcome::Planned(Plan::Trash), None),
        Assessment::Move { .. } | Assessment::Trash => act(git, trash, removed, destination)?,
    };

    let planned_destination = match outcome {
        Outcome::Planned(Plan::Move { .. }) => {
            destination.map(|destination| destination.name().clone())
        }
        _ => None,
    };
    let mut entry = Entry {
        repository: removed.display_path().to_string(),
        outcome,
        cache: CacheChange::Untouched,
        warning,
        planned_destination,
    };
    remove_cache_entry(cache, config, removed, dry_run, &mut entry)?;
    Ok(entry)
}

/// The assessment is repeated under the repository lock, so a concurrent grove
/// operation that changed the repository after the first look cannot have its
/// work moved or trashed. A moved repository whose origin could not be
/// repointed is still moved; the note explains the remote mismatch the
/// destination's own sync then reports.
fn act(
    git: &(impl RepositoryProbe + Removal),
    trash: &impl Trash,
    removed: &RemovedRepository,
    destination: Option<&RepositoryDefinition>,
) -> Result<(Outcome, Option<String>), AppError> {
    let common_directory = git.common_directory(removed.path())?;
    let _lock = git.lock_repository(&common_directory)?;
    match assess(git, removed, destination)? {
        Assessment::Move { .. } => {
            let Some(destination) = destination else {
                return Err(AppError::internal("move assessed without a destination"));
            };
            let moved = destination
                .path()
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| fs::rename(removed.path(), destination.path()));
            if let Err(err) = moved {
                let reason = BlockedReason::MoveFailed(err.to_string());
                return Ok((Outcome::Blocked { reason }, None));
            }
            let warning = match git.set_origin_url(destination.path(), destination.url()) {
                Ok(()) => None,
                Err(error) => Some(format!("origin not repointed: {}", error.demote()?)),
            };
            Ok((Outcome::Moved { to: destination.display_path().to_string() }, warning))
        }
        Assessment::Trash => Ok(match trash.trash(removed.path()) {
            Ok(()) => (Outcome::Trashed, None),
            Err(error) => {
                let reason = BlockedReason::TrashFailed(error.demote()?);
                (Outcome::Blocked { reason }, None)
            }
        }),
        Assessment::Blocked(reason) => Ok((Outcome::Blocked { reason }, None)),
        Assessment::Absent => Ok((Outcome::Absent, None)),
    }
}

/// The cache entry goes whatever state the old path is in, since it serves no
/// configured repository — unless a live repository shares its identity, as
/// when a repository merely moves between groups. A failure is a note, as
/// with seeding: the cache never affects correctness.
fn remove_cache_entry<'s>(
    cache: &dyn Fn() -> Result<&'s Store, AppError>,
    config: &ResolvedConfig,
    removed: &RemovedRepository,
    dry_run: bool,
    entry: &mut Entry,
) -> Result<(), AppError> {
    let shared = config
        .repositories()
        .iter()
        .any(|repository| repository.url().same_identity(removed.url()));
    if shared {
        return Ok(());
    }
    let result = cache().and_then(|cache| {
        if dry_run {
            Ok(if cache.is_cached(removed.url()) {
                CacheChange::WouldRemove
            } else {
                CacheChange::Untouched
            })
        } else {
            Ok(if cache.remove(removed.url())? {
                CacheChange::Removed
            } else {
                CacheChange::Untouched
            })
        }
    });
    match result {
        Ok(cache) => entry.cache = cache,
        Err(error) => {
            let message = format!("clone cache entry not removed: {}", error.demote()?);
            entry.warning = Some(match entry.warning.take() {
                Some(existing) => format!("{existing}; {message}"),
                None => message,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use tempfile::TempDir;

    use super::{Entry, Outcome, Plan, converge, run_phase};
    use crate::AppError;
    use crate::cache::Store;
    use crate::config;
    use crate::git::CommandGitClient;
    use crate::git::process_fixtures::{initialize_committed_repository, run_git};
    use crate::phases::{DiscardEvents, Summary};
    use crate::removal::BlockedReason;
    use crate::trash::Trash;

    #[derive(Default)]
    struct RecordingTrash {
        trashed: Mutex<Vec<PathBuf>>,
    }

    impl Trash for RecordingTrash {
        fn trash(&self, path: &Path) -> Result<(), AppError> {
            self.trashed.lock().unwrap().push(path.to_path_buf());
            Ok(())
        }
    }

    struct FailingTrash;

    impl Trash for FailingTrash {
        fn trash(&self, _path: &Path) -> Result<(), AppError> {
            Err(std::io::Error::other("trash unavailable").into())
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct Removing;

    /// A workspace whose `old` directory is a clean clone that grove.toml
    /// declares removed without a destination.
    fn workspace() -> (TempDir, PathBuf, config::ResolvedConfig) {
        let root = TempDir::new().unwrap();
        let origin = root.path().join("origin");
        initialize_committed_repository(&origin);
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        run_git(&workspace, &["clone", "--quiet", origin.to_str().unwrap(), "old"]);
        let config_path = workspace.join("grove.toml");
        std::fs::write(
            &config_path,
            format!("version = 2\n[removed.repos]\nold = \"{}\"\n", origin.display()),
        )
        .unwrap();
        let config = config::load(Some(&config_path)).unwrap();
        let old = workspace.join("old").canonicalize().unwrap();
        (root, old, config)
    }

    fn converge_all(
        trash: &impl Trash,
        root: &TempDir,
        config: &config::ResolvedConfig,
        dry_run: bool,
    ) -> (Vec<Entry>, Summary) {
        let git = CommandGitClient::default();
        let store = Store::with_root(root.path().join("cache"));
        let removed = config.removed().iter().collect::<Vec<_>>();
        run_phase(&removed, Removing, &DiscardEvents, |removed| {
            converge(&git, trash, &|| Ok(&store), config, removed, dry_run)
        })
        .unwrap()
    }

    #[test]
    fn trashes_a_clean_removed_repository_through_the_trash_boundary() {
        let (root, old, config) = workspace();
        let trash = RecordingTrash::default();

        let (entries, summary) = converge_all(&trash, &root, &config, false);

        assert_eq!(entries[0].outcome(), &Outcome::Trashed);
        assert_eq!(summary.count(), 1);
        assert_eq!(*trash.trashed.lock().unwrap(), [old]);
    }

    #[test]
    fn a_trash_failure_blocks_and_leaves_the_repository_in_place() {
        let (root, old, config) = workspace();

        let (entries, _) = converge_all(&FailingTrash, &root, &config, false);

        assert!(matches!(
            entries[0].outcome(),
            Outcome::Blocked { reason: BlockedReason::TrashFailed(message) }
                if message.contains("trash unavailable")
        ));
        assert!(old.join(".git").exists());
    }

    #[test]
    fn a_dry_run_plans_the_trash_without_calling_it() {
        let (root, old, config) = workspace();

        let (entries, summary) = converge_all(&FailingTrash, &root, &config, true);

        assert_eq!(entries[0].outcome(), &Outcome::Planned(Plan::Trash));
        assert_eq!(summary.count(), 0);
        assert!(old.join(".git").exists());
    }
}
