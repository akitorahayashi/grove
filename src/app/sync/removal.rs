use std::fs;

use crate::AppError;
use crate::app::AppContext;
use crate::cache::Store;
use crate::config::ResolvedConfig;
use crate::git::{Removal, RepositoryProbe};
use crate::phases::{self, EventSink, Summary as PhaseSummary};
use crate::removal::{self, Assessment, BlockedReason};
use crate::repositories::{RemovedRepository, RepositoryDefinition, RepositoryName};
use crate::trash::Trash;

use super::Phase;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RemovalPlan {
    Move { to: String },
    Trash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RemovalOutcome {
    Absent,
    Planned(RemovalPlan),
    Moved { to: String },
    Trashed,
    Blocked { reason: BlockedReason },
}

/// What happened to the clone cache entry of a removed declaration's URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CacheRemoval {
    Untouched,
    WouldRemove,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalEntry {
    repository: String,
    outcome: RemovalOutcome,
    cache: CacheRemoval,
    warning: Option<String>,
    planned_destination: Option<RepositoryName>,
}

impl RemovalEntry {
    pub fn repository(&self) -> &str {
        &self.repository
    }

    pub fn outcome(&self) -> &RemovalOutcome {
        &self.outcome
    }

    pub fn cache(&self) -> CacheRemoval {
        self.cache
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    /// The live repository a dry run plans to fill by this move.
    pub(super) fn planned_destination(&self) -> Option<&RepositoryName> {
        self.planned_destination.as_ref()
    }

    fn is_reportable(&self) -> bool {
        self.outcome != RemovalOutcome::Absent
            || self.cache != CacheRemoval::Untouched
            || self.warning.is_some()
    }
}

/// Runs before checking so a move lands before the destination would be
/// cloned afresh. Entries run sequentially: removals are rare, and moving a
/// directory must not race the probes of another entry over the same tree.
pub(super) fn phase<Z>(
    ctx: &AppContext<impl RepositoryProbe + Removal, Z, impl Trash>,
    config: &ResolvedConfig,
    removed: &[&RemovedRepository],
    dry_run: bool,
    events: &impl EventSink<Phase>,
) -> Result<(Vec<RemovalEntry>, PhaseSummary), AppError> {
    let (mut entries, summary) = phases::run_sequential(
        events,
        Phase::Removing,
        removed,
        |removed| removed.display_path(),
        |removed| remove(ctx, config, removed, dry_run),
        |entry| matches!(entry.outcome, RemovalOutcome::Moved { .. } | RemovalOutcome::Trashed),
    )?;
    entries.retain(RemovalEntry::is_reportable);
    Ok((entries, summary))
}

fn remove<Z>(
    ctx: &AppContext<impl RepositoryProbe + Removal, Z, impl Trash>,
    config: &ResolvedConfig,
    removed: &RemovedRepository,
    dry_run: bool,
) -> Result<RemovalEntry, AppError> {
    let git = ctx.git();
    let destination = removal::destination(config, removed);
    let assessment = removal::assess(git, removed, destination)?;
    let (outcome, warning) = match assessment {
        Assessment::Absent => (RemovalOutcome::Absent, None),
        Assessment::Blocked(reason) => (RemovalOutcome::Blocked { reason }, None),
        Assessment::Move { to } if dry_run => {
            (RemovalOutcome::Planned(RemovalPlan::Move { to }), None)
        }
        Assessment::Trash if dry_run => (RemovalOutcome::Planned(RemovalPlan::Trash), None),
        Assessment::Move { .. } | Assessment::Trash => act(ctx, removed, destination)?,
    };

    let planned_destination = match outcome {
        RemovalOutcome::Planned(RemovalPlan::Move { .. }) => {
            destination.map(|destination| destination.name().clone())
        }
        _ => None,
    };
    let mut entry = RemovalEntry {
        repository: removed.display_path().to_string(),
        outcome,
        cache: CacheRemoval::Untouched,
        warning,
        planned_destination,
    };
    remove_cache_entry(ctx, config, removed, dry_run, &mut entry)?;
    Ok(entry)
}

/// The assessment is repeated under the repository lock, so a concurrent grove
/// operation that changed the repository after the first look cannot have its
/// work moved or trashed. A moved repository whose origin could not be
/// repointed is still moved; the note explains the remote mismatch the
/// destination's own sync then reports.
fn act<Z>(
    ctx: &AppContext<impl RepositoryProbe + Removal, Z, impl Trash>,
    removed: &RemovedRepository,
    destination: Option<&RepositoryDefinition>,
) -> Result<(RemovalOutcome, Option<String>), AppError> {
    let git = ctx.git();
    let common_directory = git.common_directory(removed.path())?;
    let _lock = git.lock_repository(&common_directory)?;
    match removal::assess(git, removed, destination)? {
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
                return Ok((RemovalOutcome::Blocked { reason }, None));
            }
            let warning = match git.set_origin_url(destination.path(), destination.url()) {
                Ok(()) => None,
                Err(error) => Some(format!("origin not repointed: {}", error.demote()?)),
            };
            Ok((RemovalOutcome::Moved { to: destination.display_path().to_string() }, warning))
        }
        Assessment::Trash => Ok(match ctx.trash().trash(removed.path()) {
            Ok(()) => (RemovalOutcome::Trashed, None),
            Err(error) => {
                let reason = BlockedReason::TrashFailed(error.demote()?);
                (RemovalOutcome::Blocked { reason }, None)
            }
        }),
        Assessment::Blocked(reason) => Ok((RemovalOutcome::Blocked { reason }, None)),
        Assessment::Absent => Ok((RemovalOutcome::Absent, None)),
    }
}

/// The cache entry goes whatever state the old path is in, since it serves no
/// configured repository — unless a live repository shares its identity, as
/// when a repository merely moves between groups. A failure is a note, as
/// with seeding: the cache never affects correctness.
fn remove_cache_entry<Z>(
    ctx: &AppContext<impl RepositoryProbe + Removal, Z, impl Trash>,
    config: &ResolvedConfig,
    removed: &RemovedRepository,
    dry_run: bool,
    entry: &mut RemovalEntry,
) -> Result<(), AppError> {
    let shared = config
        .repositories()
        .iter()
        .any(|repository| repository.url().same_identity(removed.url()));
    if shared {
        return Ok(());
    }
    let result = ctx.cache().and_then(|cache: &Store| {
        if dry_run {
            Ok(if cache.is_cached(removed.url()) {
                CacheRemoval::WouldRemove
            } else {
                CacheRemoval::Untouched
            })
        } else {
            Ok(if cache.remove(removed.url())? {
                CacheRemoval::Removed
            } else {
                CacheRemoval::Untouched
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

    use super::{RemovalOutcome, phase};
    use crate::AppError;
    use crate::app::AppContext;
    use crate::cache::Store;
    use crate::config;
    use crate::git::CommandGitClient;
    use crate::git::process_fixtures::{initialize_committed_repository, run_git};
    use crate::phases::DiscardEvents;
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

    #[test]
    fn trashes_a_clean_removed_repository_through_the_trash_boundary() {
        let (root, old, config) = workspace();
        let ctx = AppContext::new(CommandGitClient::isolated())
            .with_trash(RecordingTrash::default())
            .with_cache(Store::with_root(root.path().join("cache")));
        let removed = config.removed().iter().collect::<Vec<_>>();

        let (entries, summary) = phase(&ctx, &config, &removed, false, &DiscardEvents).unwrap();

        assert_eq!(entries[0].outcome(), &RemovalOutcome::Trashed);
        assert_eq!(summary.count(), 1);
        assert_eq!(*ctx.trash().trashed.lock().unwrap(), [old]);
    }

    #[test]
    fn a_trash_failure_blocks_and_leaves_the_repository_in_place() {
        let (root, old, config) = workspace();
        let ctx = AppContext::new(CommandGitClient::isolated())
            .with_trash(FailingTrash)
            .with_cache(Store::with_root(root.path().join("cache")));
        let removed = config.removed().iter().collect::<Vec<_>>();

        let (entries, _) = phase(&ctx, &config, &removed, false, &DiscardEvents).unwrap();

        assert!(matches!(
            entries[0].outcome(),
            RemovalOutcome::Blocked { reason: BlockedReason::TrashFailed(message) }
                if message.contains("trash unavailable")
        ));
        assert!(old.join(".git").exists());
    }

    #[test]
    fn a_dry_run_plans_the_trash_without_calling_it() {
        let (root, old, config) = workspace();
        let ctx = AppContext::new(CommandGitClient::isolated())
            .with_trash(FailingTrash)
            .with_cache(Store::with_root(root.path().join("cache")));
        let removed = config.removed().iter().collect::<Vec<_>>();

        let (entries, summary) = phase(&ctx, &config, &removed, true, &DiscardEvents).unwrap();

        assert_eq!(entries[0].outcome(), &RemovalOutcome::Planned(super::RemovalPlan::Trash));
        assert_eq!(summary.count(), 0);
        assert!(old.join(".git").exists());
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

    fn removal_outcome(root: &TempDir, config: &config::ResolvedConfig) -> RemovalOutcome {
        let ctx = AppContext::new(CommandGitClient::isolated())
            .with_trash(RecordingTrash::default())
            .with_cache(Store::with_root(root.path().join("cache")));
        let removed = config.removed().iter().collect::<Vec<_>>();
        let (entries, _) = phase(&ctx, config, &removed, true, &DiscardEvents).unwrap();
        assert!(ctx.trash().trashed.lock().unwrap().is_empty());
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
                RemovalOutcome::Blocked { reason: BlockedReason::RemoteUrlMismatch },
                "{origin} authorized removing {declared}"
            );

            let (root, config) =
                declared_with_origin("https://example.com/old.git", Some(declared), origin);
            assert_eq!(
                removal_outcome(&root, &config),
                RemovalOutcome::Blocked { reason: BlockedReason::RemoteUrlMismatch },
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
        assert_eq!(
            removal_outcome(&root, &config),
            RemovalOutcome::Planned(super::RemovalPlan::Trash)
        );

        for origin in ["git@example.com:org/old.git", "git@example.com:org/new.git"] {
            let (root, config) = declared_with_origin(
                "git@example.com:org/old.git",
                Some("git@example.com:org/new.git"),
                origin,
            );
            assert!(
                matches!(
                    removal_outcome(&root, &config),
                    RemovalOutcome::Planned(super::RemovalPlan::Move { .. })
                ),
                "{origin} should authorize the move"
            );
        }
    }
}
