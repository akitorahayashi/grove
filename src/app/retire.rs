//! The `gv retire` use case: removing one override together with the
//! repositories and clone cache entries only it declares.

use std::path::Path;

use crate::AppError;
use crate::app::AppContext;
use crate::config::Retirement;
use crate::git::{Removal, RepositoryProbe};
use crate::phases::EventSink;
use crate::removal;
use crate::repositories::RemovedRepository;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    override_display_path: String,
    entries: Vec<removal::Entry>,
    override_outcome: OverrideOutcome,
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

    pub(crate) fn has_failures(&self) -> bool {
        matches!(self.override_outcome, OverrideOutcome::Kept | OverrideOutcome::TrashFailed(_))
    }
}

/// Retiring is one decision about the override, so every declaration is
/// assessed before anything changes, and a single blocked one leaves all of
/// them, their cache entries, and the override untouched. Acting assesses each
/// again under its repository lock; one that gained local work in between
/// still blocks and keeps the override for a later run.
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
            removal::converge(
                ctx.git(),
                ctx.trash(),
                &|| ctx.cache(),
                remaining,
                removed,
                dry_run,
                removal::BlockedCache::Keep,
            )
        }
    };

    let (assessed, _) = removal::run_phase(&retiring, Phase::Assessing, events, converge(true))?;
    if dry_run || assessed.iter().any(is_blocked) {
        let outcome = if assessed.iter().any(is_blocked) {
            OverrideOutcome::Kept
        } else {
            OverrideOutcome::WouldTrash
        };
        return Ok(report(&retirement, assessed, outcome));
    }

    let (retired, _) = removal::run_phase(&retiring, Phase::Retiring, events, converge(false))?;
    let outcome = if retired.iter().any(is_blocked) {
        OverrideOutcome::Kept
    } else {
        match ctx.trash().trash(retirement.override_path()) {
            Ok(()) => OverrideOutcome::Trashed,
            Err(error) => OverrideOutcome::TrashFailed(error.demote()?),
        }
    };
    Ok(report(&retirement, retired, outcome))
}

fn is_blocked(entry: &removal::Entry) -> bool {
    matches!(entry.outcome(), removal::Outcome::Blocked { .. })
}

fn report(
    retirement: &Retirement,
    entries: Vec<removal::Entry>,
    override_outcome: OverrideOutcome,
) -> Report {
    Report {
        override_display_path: retirement.display_path().to_string(),
        entries,
        override_outcome,
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

    /// Records every path handed to it, failing each move when `fails` is set.
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
            let git = CommandGitClient::default();
            self.store()
                .seed_from_local(&git, &self.url, &source, &mut NoopGitProgressSink)
                .unwrap();
        }

        fn retire(&self, trash: RecordingTrash, dry_run: bool) -> (Report, Vec<PathBuf>) {
            let ctx = AppContext::new(CommandGitClient::default())
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

    #[test]
    fn retires_every_repository_and_its_cache_entry_before_the_override() {
        let grove = Grove::new(&["first", "second"]);
        grove.seed_cache();

        let (report, trashed) = grove.retire(RecordingTrash::default(), false);

        assert_eq!(
            trashed,
            [
                grove.directory.join("first"),
                grove.directory.join("second"),
                grove.directory.join("grove.override.toml"),
            ]
        );
        assert_eq!(outcomes(&report), [&Outcome::Trashed, &Outcome::Trashed]);
        assert_eq!(report.entries()[0].cache(), CacheChange::Removed);
        assert_eq!(report.override_outcome(), &OverrideOutcome::Trashed);
        assert!(!grove.store().is_cached(&grove.url));
        assert!(!report.has_failures());
    }

    #[test]
    fn a_blocked_repository_leaves_the_repositories_cache_and_override_untouched() {
        let grove = Grove::new(&["first", "second"]);
        grove.seed_cache();
        fs::write(grove.directory.join("second/untracked.txt"), "local\n").unwrap();

        let (report, trashed) = grove.retire(RecordingTrash::default(), false);

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

        let (report, _) = grove.retire(RecordingTrash { fails: true, ..Default::default() }, false);

        assert!(grove.directory.join("first/.git").exists());
        assert!(grove.store().is_cached(&grove.url));
        assert_eq!(report.override_outcome(), &OverrideOutcome::Kept);
    }

    #[test]
    fn a_dry_run_plans_without_changing_anything() {
        let grove = Grove::new(&["first", "second"]);

        let (report, trashed) = grove.retire(RecordingTrash::default(), true);

        assert!(trashed.is_empty());
        assert_eq!(
            outcomes(&report),
            [&Outcome::Planned(Plan::Trash), &Outcome::Planned(Plan::Trash)]
        );
        assert_eq!(report.override_outcome(), &OverrideOutcome::WouldTrash);
        assert!(!report.has_failures());
    }

    #[test]
    fn retiring_again_after_the_repositories_are_gone_trashes_only_the_override() {
        let grove = Grove::new(&[]);

        let (report, trashed) = grove.retire(RecordingTrash::default(), false);

        assert_eq!(trashed, [grove.directory.join("grove.override.toml")]);
        assert!(report.entries().is_empty());
        assert_eq!(report.override_outcome(), &OverrideOutcome::Trashed);
    }

    #[test]
    fn a_failed_override_trash_is_reported_as_a_failure() {
        let grove = Grove::new(&[]);

        let (report, _) = grove.retire(RecordingTrash { fails: true, ..Default::default() }, false);

        assert!(matches!(
            report.override_outcome(),
            OverrideOutcome::TrashFailed(message) if message.contains("trash unavailable")
        ));
        assert!(report.has_failures());
    }
}
