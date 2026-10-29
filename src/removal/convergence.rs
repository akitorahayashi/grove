use std::fs;

use crate::AppError;
use crate::cache::Store;
use crate::config::ResolvedConfig;
use crate::git::{Removal, RepositoryProbe};
use crate::phases::{self, EventSink, Summary};
use crate::repositories::{RemoteUrl, RemovedRepository, RepositoryDefinition, RepositoryName};
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
    let (mut entries, summary) = phases::run_sequential(
        events,
        phase,
        removed,
        |removed| removed.display_path(),
        |removed| converge(removed),
        |entry| matches!(entry.outcome, Outcome::Moved { .. } | Outcome::Trashed),
    )?;
    entries.retain(Entry::is_reportable);
    Ok((entries, summary))
}

/// `config` is the configuration that stays in force: destinations resolve
/// against its repositories, and a cache entry one of them shares is kept.
/// Given `cache`, the entry for the declaration's URL goes whatever the old
/// path's state, since that URL serves no configured repository; without it the
/// caller settles the cache.
pub(crate) fn converge<'s>(
    git: &(impl RepositoryProbe + Removal),
    trash: &impl Trash,
    cache: Option<&dyn Fn() -> Result<&'s Store, AppError>>,
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
    if let Some(cache) = cache {
        remove_cache_entry(cache, config, removed, dry_run, &mut entry)?;
    }
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

/// A live repository sharing the entry's identity keeps it, as when a
/// repository merely moves between groups. A failure is a note, as with
/// seeding: the cache never affects correctness.
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
    match cache().and_then(|cache| drop_cache_entry(cache, removed.url(), dry_run)) {
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

/// Removes the clone cache entry for `url`, or in a dry run reports whether
/// there is one to remove.
pub(crate) fn drop_cache_entry(
    cache: &Store,
    url: &RemoteUrl,
    dry_run: bool,
) -> Result<CacheChange, AppError> {
    if dry_run {
        return Ok(if cache.is_cached(url) {
            CacheChange::WouldRemove
        } else {
            CacheChange::Untouched
        });
    }
    Ok(if cache.remove(url)? { CacheChange::Removed } else { CacheChange::Untouched })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use tempfile::TempDir;

    use super::{CacheChange, Entry, Outcome, Plan, converge, run_phase};
    use crate::AppError;
    use crate::cache::Store;
    use crate::config;
    use crate::git::process_fixtures::{initialize_committed_repository, run_git};
    use crate::git::{CommandGitClient, NoopGitProgressSink};
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
        with_cache: bool,
    ) -> (Vec<Entry>, Summary) {
        let git = CommandGitClient::isolated();
        let store = Store::with_root(root.path().join("cache"));
        let cache = || Ok(&store);
        let cache: Option<&dyn Fn() -> _> = with_cache.then_some(&cache);
        let removed = config.removed().iter().collect::<Vec<_>>();
        run_phase(&removed, Removing, &DiscardEvents, |removed| {
            converge(&git, trash, cache, config, removed, dry_run)
        })
        .unwrap()
    }

    fn seed_cache(root: &TempDir, config: &config::ResolvedConfig, old: &Path) -> Store {
        let store = Store::with_root(root.path().join("cache"));
        let url = config.removed()[0].url();
        let git = CommandGitClient::isolated();
        store.seed_from_local(&git, url, &old.join(".git"), &mut NoopGitProgressSink).unwrap();
        store
    }

    #[test]
    fn trashes_a_clean_removed_repository_through_the_trash_boundary() {
        let (root, old, config) = workspace();
        let trash = RecordingTrash::default();

        let (entries, summary) = converge_all(&trash, &root, &config, false, true);

        assert_eq!(entries[0].outcome(), &Outcome::Trashed);
        assert_eq!(summary.count(), 1);
        assert_eq!(*trash.trashed.lock().unwrap(), [old]);
    }

    #[test]
    fn a_trash_failure_blocks_and_leaves_the_repository_in_place() {
        let (root, old, config) = workspace();

        let (entries, _) = converge_all(&FailingTrash, &root, &config, false, true);

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

        let (entries, summary) = converge_all(&FailingTrash, &root, &config, true, true);

        assert_eq!(entries[0].outcome(), &Outcome::Planned(Plan::Trash));
        assert_eq!(summary.count(), 0);
        assert!(old.join(".git").exists());
    }

    #[test]
    fn a_blocked_path_drops_its_cache_entry_only_when_given_the_cache() {
        let (root, old, config) = workspace();
        let store = seed_cache(&root, &config, &old);

        let (untouched, _) = converge_all(&FailingTrash, &root, &config, false, false);

        assert_eq!(untouched[0].cache(), CacheChange::Untouched);
        assert!(store.is_cached(config.removed()[0].url()));

        let (removed, _) = converge_all(&FailingTrash, &root, &config, false, true);

        assert_eq!(removed[0].cache(), CacheChange::Removed);
        assert!(!store.is_cached(config.removed()[0].url()));
    }

    /// A clean clone at `old`, declared removed with `declared` and
    /// optionally moving to a live `new` repository at `moved_to`, whose
    /// origin is then repointed at `origin`.
    fn declared_with_origin(
        declared: &str,
        moved_to: Option<&str>,
        origin: &str,
    ) -> (TempDir, config::ResolvedConfig) {
        let (root, old, _) = workspace();
        run_git(&old, &["remote", "set-url", "origin", origin]);
        let mut contents = match moved_to {
            Some(_) => format!(
                "version = 2\n[removed.repos]\nold = {{ url = \"{declared}\", moved_to = \"new\" }}\n"
            ),
            None => format!("version = 2\n[removed.repos]\nold = \"{declared}\"\n"),
        };
        if let Some(url) = moved_to {
            contents.push_str(&format!("[repos]\nnew = \"{url}\"\n"));
        }
        let config_path = root.path().join("workspace/grove.toml");
        std::fs::write(&config_path, contents).unwrap();
        let config = config::load(Some(&config_path)).unwrap();
        (root, config)
    }

    fn removal_outcome(root: &TempDir, config: &config::ResolvedConfig) -> Outcome {
        let trash = RecordingTrash::default();
        let (entries, _) = converge_all(&trash, root, config, true, false);
        assert!(trash.trashed.lock().unwrap().is_empty());
        entries[0].outcome().clone()
    }

    #[test]
    fn a_shared_cache_identity_never_authorizes_removal() {
        for (declared, origin) in [
            ("alice@example.com:repo.git", "bob@example.com:repo.git"),
            ("https://example.com/org/repo.git", "git@example.com:org/repo.git"),
            ("example.com:repo.git", "example.com:/repo.git"),
        ] {
            let (root, config) = declared_with_origin(declared, None, origin);
            assert_eq!(
                removal_outcome(&root, &config),
                Outcome::Blocked { reason: BlockedReason::RemoteUrlMismatch },
                "{origin} authorized removing {declared}"
            );

            let (root, config) =
                declared_with_origin("https://example.com/old.git", Some(declared), origin);
            assert_eq!(
                removal_outcome(&root, &config),
                Outcome::Blocked { reason: BlockedReason::RemoteUrlMismatch },
                "{origin} authorized moving to {declared}"
            );
        }
    }

    #[test]
    fn an_exact_match_to_either_configured_url_authorizes_removal() {
        let (root, config) = declared_with_origin(
            "git@example.com:org/old.git",
            None,
            "git@example.com:org/old.git",
        );
        assert_eq!(removal_outcome(&root, &config), Outcome::Planned(Plan::Trash));

        for origin in ["git@example.com:org/old.git", "git@example.com:org/new.git"] {
            let (root, config) = declared_with_origin(
                "git@example.com:org/old.git",
                Some("git@example.com:org/new.git"),
                origin,
            );
            assert!(
                matches!(removal_outcome(&root, &config), Outcome::Planned(Plan::Move { .. })),
                "{origin} should authorize the move"
            );
        }
    }
}
