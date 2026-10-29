//! The `gv replace` use case: converging the disk from the root configuration
//! to a candidate, offline, and only then writing the candidate in its place,
//! so the file stays this machine's record of the last converged catalog.

mod repoint;

use std::io;
use std::path::{Path, PathBuf};

use crate::AppError;
use crate::app::AppContext;
use crate::config::{Persistence, Replacement, ResolvedConfig};
use crate::git::{Removal, RepositoryProbe};
use crate::phases::DiscardEvents;
use crate::removal::{self, CacheDrop};
use crate::repositories::RemovedRepository;
use crate::trash::Trash;

pub(crate) use repoint::{Entry as RepointEntry, Outcome as RepointOutcome};

/// What became of the destination file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigurationOutcome {
    /// A blocked change keeps the file, which still describes the disk.
    Kept,
    WouldWrite,
    Current,
    Written,
    DurabilityUnconfirmed(String),
    WriteFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    destination: PathBuf,
    removals: Vec<removal::Entry>,
    repoints: Vec<RepointEntry>,
    configuration: ConfigurationOutcome,
    cache: Vec<CacheDrop>,
    not_cloned: usize,
}

impl Report {
    pub(crate) fn destination(&self) -> &Path {
        &self.destination
    }

    /// The removed declarations acted on, or only assessed when a dry run or
    /// a blocked one stopped the replacement before any change.
    pub(crate) fn removals(&self) -> &[removal::Entry] {
        &self.removals
    }

    pub(crate) fn repoints(&self) -> &[RepointEntry] {
        &self.repoints
    }

    pub(crate) fn configuration(&self) -> &ConfigurationOutcome {
        &self.configuration
    }

    pub(crate) fn cache(&self) -> &[CacheDrop] {
        &self.cache
    }

    /// Live repositories of the candidate's configuration with nothing at
    /// their path, apart from the destinations planned moves fill.
    pub(crate) fn not_cloned(&self) -> usize {
        self.not_cloned
    }

    pub(crate) fn has_failures(&self) -> bool {
        matches!(
            self.configuration,
            ConfigurationOutcome::Kept
                | ConfigurationOutcome::DurabilityUnconfirmed(_)
                | ConfigurationOutcome::WriteFailed(_)
        ) || is_blocked(&self.removals, &self.repoints)
    }
}

/// Replacing is one decision about the file, so everything is assessed before
/// anything changes, and a single blocked path leaves the disk, the cache, and
/// the file untouched. Acting assesses each path again under its repository
/// lock; one that gained local work in between still blocks and keeps the
/// file, and a later run skips what is already done. Cache entries go only
/// after the file, since until then it still declares their repositories.
pub(crate) fn execute<Z>(
    ctx: &AppContext<impl RepositoryProbe + Removal, Z, impl Trash>,
    config_path: Option<&Path>,
    candidate: &Path,
    dry_run: bool,
) -> Result<Report, AppError> {
    ctx.git().verify_available()?;
    let replacement = Replacement::open(config_path, candidate, dry_run)?;
    let removals = replacement.removals().collect::<Vec<_>>();
    let prospective = replacement.prospective();
    let converge = |dry_run| {
        move |removed: &RemovedRepository| {
            removal::converge(ctx.git(), ctx.trash(), None, prospective, removed, dry_run)
        }
    };
    let repoints = |dry_run| {
        replacement
            .repoints()
            .iter()
            .filter_map(|repoint| repoint::converge(ctx.git(), repoint, dry_run).transpose())
            .collect::<Result<Vec<_>, _>>()
    };
    let drop_cache = |dry_run| {
        removal::drop_cache_entries(&|| ctx.cache(), replacement.removals(), prospective, dry_run)
    };
    let report = |removals, repoints, configuration, cache, not_cloned| Report {
        destination: replacement.destination().to_path_buf(),
        removals,
        repoints,
        configuration,
        cache,
        not_cloned,
    };

    let (assessed, _) = removal::run_phase(&removals, (), &DiscardEvents, converge(true))?;
    let planned = repoints(true)?;
    if is_blocked(&assessed, &planned) {
        return Ok(report(assessed, planned, ConfigurationOutcome::Kept, Vec::new(), 0));
    }
    // Counted before acting, so a path that cannot be inspected fails the run
    // while nothing has changed rather than after the write.
    let not_cloned = not_cloned(prospective, &assessed)?;
    if dry_run {
        let configuration = if replacement.is_current() {
            ConfigurationOutcome::Current
        } else {
            ConfigurationOutcome::WouldWrite
        };
        let cache = drop_cache(true)?;
        return Ok(report(assessed, planned, configuration, cache, not_cloned));
    }

    let (converged, _) = removal::run_phase(&removals, (), &DiscardEvents, converge(false))?;
    let repointed = repoints(false)?;
    if is_blocked(&converged, &repointed) {
        return Ok(report(converged, repointed, ConfigurationOutcome::Kept, Vec::new(), 0));
    }
    let configuration = write(&replacement)?;
    let cache = match configuration {
        ConfigurationOutcome::Written | ConfigurationOutcome::DurabilityUnconfirmed(_) => {
            drop_cache(false)?
        }
        _ => Vec::new(),
    };
    Ok(report(converged, repointed, configuration, cache, not_cloned))
}

/// Identical bytes are left alone, keeping the file's identity and timestamps.
fn write(replacement: &Replacement) -> Result<ConfigurationOutcome, AppError> {
    if replacement.is_current() {
        return Ok(ConfigurationOutcome::Current);
    }
    Ok(match replacement.write() {
        Ok(Persistence::Durable) => ConfigurationOutcome::Written,
        Ok(Persistence::DurabilityUnconfirmed(message)) => {
            ConfigurationOutcome::DurabilityUnconfirmed(message)
        }
        Err(error) => ConfigurationOutcome::WriteFailed(error.demote()?),
    })
}

fn not_cloned(config: &ResolvedConfig, removals: &[removal::Entry]) -> Result<usize, AppError> {
    let mut count = 0;
    for repository in config.repositories() {
        let filled =
            removals.iter().any(|entry| entry.planned_destination() == Some(repository.name()));
        if !filled && !exists(repository.path())? {
            count += 1;
        }
    }
    Ok(count)
}

fn exists(path: &Path) -> Result<bool, AppError> {
    match path.symlink_metadata() {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn is_blocked(removals: &[removal::Entry], repoints: &[RepointEntry]) -> bool {
    removals.iter().any(|entry| matches!(entry.outcome(), removal::Outcome::Blocked { .. }))
        || repoints.iter().any(|entry| matches!(entry.outcome(), RepointOutcome::Blocked { .. }))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use tempfile::TempDir;

    use super::{ConfigurationOutcome, Report, execute};
    use crate::AppError;
    use crate::app::AppContext;
    use crate::cache::Store;
    use crate::git::process_fixtures::{initialize_committed_repository, run_git};
    use crate::git::{CommandGitClient, NoopGitProgressSink};
    use crate::removal::{CacheChange, Outcome, Plan};
    use crate::repositories::RemoteUrl;
    use crate::trash::Trash;

    /// Records every path handed to it, failing every move when `fails`.
    #[derive(Default)]
    struct RecordingTrash {
        fails: bool,
        paths: Mutex<Vec<PathBuf>>,
    }

    impl Trash for RecordingTrash {
        fn trash(&self, path: &Path) -> Result<(), AppError> {
            self.paths.lock().unwrap().push(path.to_path_buf());
            if self.fails {
                return Err(std::io::Error::other("trash unavailable").into());
            }
            Ok(())
        }
    }

    /// A grove whose grove.toml declares `old`, a clean clone with a seeded
    /// cache entry, and a candidate that no longer declares it.
    struct Grove {
        root: TempDir,
        directory: PathBuf,
        url: RemoteUrl,
        current: String,
    }

    impl Grove {
        fn new() -> Self {
            let root = TempDir::new().unwrap();
            let origin = root.path().join("origin");
            initialize_committed_repository(&origin);
            let directory = root.path().join("workspace");
            fs::create_dir_all(&directory).unwrap();
            run_git(&directory, &["clone", "--quiet", origin.to_str().unwrap(), "old"]);
            let current = format!("version = 2\n[repos]\nold = \"{}\"\n", origin.display());
            fs::write(directory.join("grove.toml"), &current).unwrap();
            fs::write(root.path().join("candidate.toml"), "version = 2\n").unwrap();
            let url = RemoteUrl::new(&origin.display().to_string()).unwrap();
            let grove = Self { root, directory: directory.canonicalize().unwrap(), url, current };
            grove
                .store()
                .seed_from_local(
                    &CommandGitClient::isolated(),
                    &grove.url,
                    &grove.directory.join("old/.git"),
                    &mut NoopGitProgressSink,
                )
                .unwrap();
            grove
        }

        fn store(&self) -> Store {
            Store::with_root(self.root.path().join("cache"))
        }

        fn replace(&self, trash: RecordingTrash, dry_run: bool) -> (Report, Vec<PathBuf>) {
            let ctx = AppContext::new(CommandGitClient::isolated())
                .with_trash(trash)
                .with_cache(self.store());
            let config = self.directory.join("grove.toml");
            let candidate = self.root.path().join("candidate.toml");
            let report = execute(&ctx, Some(&config), &candidate, dry_run).unwrap();
            let paths = ctx.trash().paths.lock().unwrap().clone();
            (report, paths)
        }

        fn configuration(&self) -> String {
            fs::read_to_string(self.directory.join("grove.toml")).unwrap()
        }
    }

    fn outcomes(report: &Report) -> Vec<&Outcome> {
        report.removals().iter().map(|entry| entry.outcome()).collect()
    }

    fn cache_changes(report: &Report) -> Vec<(String, CacheChange)> {
        report.cache().iter().map(|entry| (entry.identity().to_string(), entry.change())).collect()
    }

    #[test]
    fn trashes_a_leaving_repository_before_writing_and_dropping_its_cache_entry() {
        let grove = Grove::new();

        let (report, trashed) = grove.replace(RecordingTrash::default(), false);

        assert_eq!(trashed, [grove.directory.join("old")]);
        assert_eq!(outcomes(&report), [&Outcome::Trashed]);
        assert_eq!(report.configuration(), &ConfigurationOutcome::Written);
        assert_eq!(grove.configuration(), "version = 2\n");
        assert_eq!(cache_changes(&report), [(grove.url.identity(), CacheChange::Removed)]);
        assert!(!grove.store().is_cached(&grove.url));
        assert!(!report.has_failures());
    }

    #[test]
    fn a_blocked_repository_leaves_the_disk_cache_and_configuration_untouched() {
        let grove = Grove::new();
        fs::write(grove.directory.join("old/untracked.txt"), "local\n").unwrap();

        let (report, trashed) = grove.replace(RecordingTrash::default(), false);

        assert!(trashed.is_empty());
        assert!(matches!(outcomes(&report)[..], [Outcome::Blocked { .. }]));
        assert_eq!(report.configuration(), &ConfigurationOutcome::Kept);
        assert_eq!(grove.configuration(), grove.current);
        assert!(grove.store().is_cached(&grove.url));
        assert!(report.has_failures());
    }

    #[test]
    fn a_failed_trash_keeps_the_configuration_and_cache_entry() {
        let grove = Grove::new();

        let (report, _) =
            grove.replace(RecordingTrash { fails: true, ..Default::default() }, false);

        assert!(grove.directory.join("old/.git").exists());
        assert_eq!(report.configuration(), &ConfigurationOutcome::Kept);
        assert_eq!(grove.configuration(), grove.current);
        assert!(grove.store().is_cached(&grove.url));
        assert!(report.has_failures());
    }

    #[test]
    fn a_dry_run_plans_without_changing_anything() {
        let grove = Grove::new();

        let (report, trashed) = grove.replace(RecordingTrash::default(), true);

        assert!(trashed.is_empty());
        assert_eq!(outcomes(&report), [&Outcome::Planned(Plan::Trash)]);
        assert_eq!(report.configuration(), &ConfigurationOutcome::WouldWrite);
        assert_eq!(cache_changes(&report), [(grove.url.identity(), CacheChange::WouldRemove)]);
        assert_eq!(grove.configuration(), grove.current);
        assert!(grove.store().is_cached(&grove.url));
        assert!(!report.has_failures());
    }

    #[test]
    fn replacing_again_once_the_repository_is_gone_only_writes() {
        let grove = Grove::new();
        fs::remove_dir_all(grove.directory.join("old")).unwrap();

        let (report, trashed) = grove.replace(RecordingTrash::default(), false);

        assert!(trashed.is_empty());
        assert!(report.removals().is_empty());
        assert_eq!(report.configuration(), &ConfigurationOutcome::Written);
        assert_eq!(report.not_cloned(), 0);
    }
}
