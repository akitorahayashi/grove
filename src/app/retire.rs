//! The `gv retire` use case: removing one override together with the
//! repositories and clone cache entries only it declares.

use std::path::Path;

use crate::AppError;
use crate::app::AppContext;
use crate::config::Retirement;
use crate::git::{Removal, RepositoryProbe};
use crate::phases::EventSink;
use crate::removal::{self, CacheChange};
use crate::repositories::{RemoteUrl, RemovedRepository};
use crate::trash::Trash;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Assessing,
    Retiring,
}

/// What became of the override file itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OverrideOutcome {
    /// A blocked repository keeps the override, so a later run can retire it.
    Kept,
    WouldTrash,
    Trashed,
    TrashFailed(String),
}

/// A clone cache entry that only the retired declarations' URLs name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheRemoval {
    identity: String,
    change: CacheChange,
    warning: Option<String>,
}

impl CacheRemoval {
    pub(crate) fn identity(&self) -> &str {
        &self.identity
    }

    pub(crate) fn change(&self) -> CacheChange {
        self.change
    }

    pub(crate) fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    override_display_path: String,
    entries: Vec<removal::Entry>,
    override_outcome: OverrideOutcome,
    cache: Vec<CacheRemoval>,
}

impl Report {
    pub(crate) fn override_display_path(&self) -> &str {
        &self.override_display_path
    }

    /// The retiring declarations acted on, or only assessed when a dry run or
    /// a blocked declaration stopped retirement before any change.
    pub(crate) fn entries(&self) -> &[removal::Entry] {
        &self.entries
    }

    pub(crate) fn override_outcome(&self) -> &OverrideOutcome {
        &self.override_outcome
    }

    pub(crate) fn cache(&self) -> &[CacheRemoval] {
        &self.cache
    }

    pub(crate) fn has_failures(&self) -> bool {
        matches!(self.override_outcome, OverrideOutcome::Kept | OverrideOutcome::TrashFailed(_))
    }
}

/// Retiring is one decision about the override, so every declaration is
/// assessed before anything changes, and a single blocked one leaves all of
/// them, their cache entries, and the override untouched. Acting assesses each
/// again under its repository lock; one that gained local work in between
/// still blocks and keeps the override for a later run. Cache entries go only
/// after the override, since until then it still declares their repositories.
pub(crate) fn execute<Z>(
    ctx: &AppContext<impl RepositoryProbe + Removal, Z, impl Trash>,
    config_path: Option<&Path>,
    target: Option<&Path>,
    dry_run: bool,
    events: &impl EventSink<Phase>,
) -> Result<Report, AppError> {
    ctx.git().verify_available()?;
    let retirement = Retirement::open(config_path, target, dry_run)?;
    let retiring = retirement.retiring().iter().collect::<Vec<_>>();
    let remaining = retirement.remaining();
    let converge = |dry_run| {
        move |removed: &RemovedRepository| {
            removal::converge(ctx.git(), ctx.trash(), None, remaining, removed, dry_run)
        }
    };

    let (assessed, _) = removal::run_phase(&retiring, Phase::Assessing, events, converge(true))?;
    if assessed.iter().any(is_blocked) {
        return Ok(report(&retirement, assessed, OverrideOutcome::Kept, Vec::new()));
    }
    if dry_run {
        let cache = drop_cache(ctx, &retirement, true)?;
        return Ok(report(&retirement, assessed, OverrideOutcome::WouldTrash, cache));
    }

    let (retired, _) = removal::run_phase(&retiring, Phase::Retiring, events, converge(false))?;
    if retired.iter().any(is_blocked) {
        return Ok(report(&retirement, retired, OverrideOutcome::Kept, Vec::new()));
    }
    if let Err(error) = ctx.trash().trash(retirement.override_path()) {
        let outcome = OverrideOutcome::TrashFailed(error.demote()?);
        return Ok(report(&retirement, retired, outcome, Vec::new()));
    }
    let cache = drop_cache(ctx, &retirement, false)?;
    Ok(report(&retirement, retired, OverrideOutcome::Trashed, cache))
}

/// Each identity the retired URLs name goes once, unless a remaining
/// repository shares it. A failure is a note: the cache never affects
/// correctness.
fn drop_cache<Z>(
    ctx: &AppContext<impl RepositoryProbe + Removal, Z, impl Trash>,
    retirement: &Retirement,
    dry_run: bool,
) -> Result<Vec<CacheRemoval>, AppError> {
    let mut urls: Vec<&RemoteUrl> = Vec::new();
    for removed in retirement.retiring() {
        let url = removed.url();
        let shared = retirement
            .remaining()
            .repositories()
            .iter()
            .any(|repository| repository.url().same_identity(url));
        if !shared && !urls.iter().any(|seen| seen.same_identity(url)) {
            urls.push(url);
        }
    }

    let mut removals = Vec::new();
    for url in urls {
        let (change, warning) =
            match ctx.cache().and_then(|cache| removal::drop_cache_entry(cache, url, dry_run)) {
                Ok(CacheChange::Untouched) => continue,
                Ok(change) => (change, None),
                Err(error) => (CacheChange::Untouched, Some(error.demote()?)),
            };
        removals.push(CacheRemoval { identity: url.identity(), change, warning });
    }
    Ok(removals)
}

fn is_blocked(entry: &removal::Entry) -> bool {
    matches!(entry.outcome(), removal::Outcome::Blocked { .. })
}

fn report(
    retirement: &Retirement,
    entries: Vec<removal::Entry>,
    override_outcome: OverrideOutcome,
    cache: Vec<CacheRemoval>,
) -> Report {
    Report {
        override_display_path: retirement.display_path().to_string(),
        entries,
        override_outcome,
        cache,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use tempfile::TempDir;

    use super::{OverrideOutcome, Report, execute};
    use crate::AppError;
    use crate::app::AppContext;
    use crate::cache::Store;
    use crate::git::process_fixtures::{initialize_committed_repository, run_git};
    use crate::git::{CommandGitClient, NoopGitProgressSink};
    use crate::phases::DiscardEvents;
    use crate::removal::{CacheChange, Outcome, Plan};
    use crate::repositories::RemoteUrl;
    use crate::trash::Trash;

    /// Records every path handed to it, failing the moves `fails` selects.
    struct RecordingTrash {
        fails: fn(&Path) -> bool,
        paths: Mutex<Vec<PathBuf>>,
    }

    impl RecordingTrash {
        fn new() -> Self {
            Self::failing(|_| false)
        }

        fn failing(fails: fn(&Path) -> bool) -> Self {
            Self { fails, paths: Mutex::default() }
        }
    }

    impl Trash for RecordingTrash {
        fn trash(&self, path: &Path) -> Result<(), AppError> {
            self.paths.lock().unwrap().push(path.to_path_buf());
            if (self.fails)(path) {
                return Err(std::io::Error::other("trash unavailable").into());
            }
            Ok(())
        }
    }

    /// A grove whose override alone declares `first` and `second`, with the
    /// named ones cloned from a shared origin.
    struct Grove {
        root: TempDir,
        directory: PathBuf,
        url: RemoteUrl,
    }

    impl Grove {
        fn new(cloned: &[&str]) -> Self {
            let root = TempDir::new().unwrap();
            let origin = root.path().join("origin");
            initialize_committed_repository(&origin);
            let directory = root.path().join("workspace");
            fs::create_dir_all(&directory).unwrap();
            for name in cloned {
                run_git(&directory, &["clone", "--quiet", origin.to_str().unwrap(), name]);
            }
            fs::write(directory.join("grove.toml"), "version = 2\n").unwrap();
            fs::write(
                directory.join("grove.override.toml"),
                format!("[repos]\nfirst = \"{0}\"\nsecond = \"{0}\"\n", origin.display()),
            )
            .unwrap();
            let directory = directory.canonicalize().unwrap();
            let url = RemoteUrl::new(&origin.display().to_string()).unwrap();
            Self { root, directory, url }
        }

        fn store(&self) -> Store {
            Store::with_root(self.root.path().join("cache"))
        }

        fn seed_cache(&self) {
            let source = self.directory.join("first/.git");
            let git = CommandGitClient::isolated();
            self.store()
                .seed_from_local(&git, &self.url, &source, &mut NoopGitProgressSink)
                .unwrap();
        }

        fn retire(&self, trash: RecordingTrash, dry_run: bool) -> (Report, Vec<PathBuf>) {
            let ctx = AppContext::new(CommandGitClient::isolated())
                .with_trash(trash)
                .with_cache(self.store());
            let config = self.directory.join("grove.toml");
            let report = execute(&ctx, Some(&config), None, dry_run, &DiscardEvents).unwrap();
            let paths = ctx.trash().paths.lock().unwrap().clone();
            (report, paths)
        }
    }

    fn outcomes(report: &Report) -> Vec<&Outcome> {
        report.entries().iter().map(|entry| entry.outcome()).collect()
    }

    fn cache_changes(report: &Report) -> Vec<(String, CacheChange)> {
        report.cache().iter().map(|entry| (entry.identity().to_string(), entry.change())).collect()
    }

    #[test]
    fn retires_every_repository_and_its_cache_entry_before_the_override() {
        let grove = Grove::new(&["first", "second"]);
        grove.seed_cache();

        let (report, trashed) = grove.retire(RecordingTrash::new(), false);

        assert_eq!(
            trashed,
            [
                grove.directory.join("first"),
                grove.directory.join("second"),
                grove.directory.join("grove.override.toml"),
            ]
        );
        assert_eq!(outcomes(&report), [&Outcome::Trashed, &Outcome::Trashed]);
        assert_eq!(report.override_outcome(), &OverrideOutcome::Trashed);
        assert_eq!(cache_changes(&report), [(grove.url.identity(), CacheChange::Removed)]);
        assert!(!grove.store().is_cached(&grove.url));
        assert!(!report.has_failures());
    }

    #[test]
    fn a_blocked_repository_leaves_the_repositories_cache_and_override_untouched() {
        let grove = Grove::new(&["first", "second"]);
        grove.seed_cache();
        fs::write(grove.directory.join("second/untracked.txt"), "local\n").unwrap();

        let (report, trashed) = grove.retire(RecordingTrash::new(), false);

        assert!(trashed.is_empty());
        assert!(matches!(
            outcomes(&report)[..],
            [Outcome::Planned(Plan::Trash), Outcome::Blocked { .. }]
        ));
        assert_eq!(report.override_outcome(), &OverrideOutcome::Kept);
        assert!(grove.store().is_cached(&grove.url));
        assert!(report.has_failures());
    }

    #[test]
    fn a_repository_that_fails_to_trash_keeps_its_cache_entry_and_the_override() {
        let grove = Grove::new(&["first", "second"]);
        grove.seed_cache();

        let (report, _) = grove.retire(RecordingTrash::failing(|_| true), false);

        assert!(grove.directory.join("first/.git").exists());
        assert!(grove.store().is_cached(&grove.url));
        assert_eq!(report.override_outcome(), &OverrideOutcome::Kept);
    }

    #[test]
    fn a_partial_failure_keeps_the_cache_entry_a_remaining_checkout_still_uses() {
        let grove = Grove::new(&["first", "second"]);
        grove.seed_cache();

        let trash = RecordingTrash::failing(|path| path.ends_with("second"));
        let (report, trashed) = grove.retire(trash, false);

        assert_eq!(trashed[0], grove.directory.join("first"));
        assert!(grove.directory.join("second/.git").exists());
        assert!(grove.store().is_cached(&grove.url));
        assert_eq!(report.override_outcome(), &OverrideOutcome::Kept);
    }

    #[test]
    fn a_dry_run_plans_without_changing_anything() {
        let grove = Grove::new(&["first", "second"]);
        grove.seed_cache();

        let (report, trashed) = grove.retire(RecordingTrash::new(), true);

        assert!(trashed.is_empty());
        assert_eq!(
            outcomes(&report),
            [&Outcome::Planned(Plan::Trash), &Outcome::Planned(Plan::Trash)]
        );
        assert_eq!(report.override_outcome(), &OverrideOutcome::WouldTrash);
        assert_eq!(cache_changes(&report), [(grove.url.identity(), CacheChange::WouldRemove)]);
        assert!(grove.store().is_cached(&grove.url));
        assert!(!report.has_failures());
    }

    #[test]
    fn retiring_again_after_the_repositories_are_gone_trashes_only_the_override() {
        let grove = Grove::new(&[]);

        let (report, trashed) = grove.retire(RecordingTrash::new(), false);

        assert_eq!(trashed, [grove.directory.join("grove.override.toml")]);
        assert!(report.entries().is_empty());
        assert_eq!(report.override_outcome(), &OverrideOutcome::Trashed);
    }

    #[test]
    fn a_failed_override_trash_is_reported_as_a_failure() {
        let grove = Grove::new(&[]);

        let (report, _) = grove.retire(RecordingTrash::failing(|_| true), false);

        assert!(matches!(
            report.override_outcome(),
            OverrideOutcome::TrashFailed(message) if message.contains("trash unavailable")
        ));
        assert!(report.has_failures());
    }
}
