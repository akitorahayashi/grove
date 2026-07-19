use std::path::Path;
use std::time::Instant;

use crate::AppError;
use crate::app::AppContext;
use crate::config;
use crate::git::GitClient;
use crate::repositories::{RepositoryDefinition, select_repositories};

mod check;
mod events;
mod prepare;
mod report;
mod update;
mod workers;

pub use events::Phase;
pub(crate) use events::{Event, EventSink};
pub use report::{
    BlockedReason, Entry, Outcome, PhaseSummaries, PhaseSummary, Plan, Report, SkippedReason,
};

use events::DiscardEvents;

pub fn execute(
    ctx: &AppContext<impl GitClient>,
    config_path: Option<&Path>,
    targets: &[String],
    dry_run: bool,
) -> Result<Report, AppError> {
    execute_with_events(ctx, config_path, targets, dry_run, &DiscardEvents)
}

pub(crate) fn execute_with_events(
    ctx: &AppContext<impl GitClient>,
    config_path: Option<&Path>,
    targets: &[String],
    dry_run: bool,
    events: &impl EventSink,
) -> Result<Report, AppError> {
    ctx.git().verify_available()?;
    let config = config::load(config_path)?;
    let repositories = select_repositories(config.repositories(), targets)?;
    let parallelism = std::thread::available_parallelism()?.get();
    let started = Instant::now();
    let total = repositories.len();
    let mut entries = std::iter::repeat_with(|| None).take(total).collect::<Vec<_>>();

    let (decisions, checked) = check_phase(ctx.git(), &repositories, parallelism, dry_run, events)?;

    let mut preparations = Vec::new();
    for (index, (repository, decision)) in repositories.iter().copied().zip(decisions).enumerate() {
        match decision {
            check::Decision::Entry(entry) => entries[index] = Some(entry),
            check::Decision::Clone => {
                preparations.push(prepare::Task::Clone { index, repository });
            }
            check::Decision::Fetch { common_directory, default_branch, current_branch } => {
                preparations.push(prepare::Task::Fetch {
                    index,
                    repository,
                    common_directory,
                    default_branch,
                    current_branch,
                });
            }
        }
    }

    let (updates, prepared) =
        prepare_phase(ctx.git(), &preparations, &mut entries, parallelism, events);
    let updated = update_phase(ctx.git(), &updates, &mut entries, parallelism, events);

    let entries = entries
        .into_iter()
        .map(|entry| entry.expect("every selected repository should produce an outcome"))
        .collect();
    let phases = PhaseSummaries::new(checked, prepared, updated);
    Ok(Report::new(entries, started.elapsed(), phases))
}

fn check_phase(
    git: &impl GitClient,
    repositories: &[&RepositoryDefinition],
    parallelism: usize,
    dry_run: bool,
    events: &impl EventSink,
) -> Result<(Vec<check::Decision>, PhaseSummary), AppError> {
    events.emit(Event::PhaseStarted { phase: Phase::Checking, total: repositories.len() });
    let started = Instant::now();
    let results = workers::map(repositories, parallelism, |repository| {
        emit_repository_started(events, repository, Phase::Checking);
        let result = check::repository(git, repository, dry_run);
        emit_repository_finished(events, repository, Phase::Checking);
        result
    });
    let elapsed = started.elapsed();

    let mut decisions = Vec::with_capacity(results.len());
    for result in results {
        match result {
            Ok(decision) => decisions.push(decision),
            Err(err) => {
                events.emit(Event::PhaseFailed { phase: Phase::Checking });
                return Err(err);
            }
        }
    }

    let summary = PhaseSummary::new(decisions.len(), elapsed);
    events.emit(Event::PhaseCompleted { phase: Phase::Checking, summary });
    Ok((decisions, summary))
}

fn prepare_phase<'a>(
    git: &impl GitClient,
    tasks: &[prepare::Task<'a>],
    entries: &mut [Option<Entry>],
    parallelism: usize,
    events: &impl EventSink,
) -> (Vec<update::Task<'a>>, PhaseSummary) {
    if tasks.is_empty() {
        return (Vec::new(), PhaseSummary::default());
    }

    events.emit(Event::PhaseStarted { phase: Phase::Preparing, total: tasks.len() });
    let started = Instant::now();
    let completions = workers::map_keyed(
        tasks,
        parallelism,
        |task| task.resource().to_path_buf(),
        |task| {
            emit_repository_started(events, task.repository(), Phase::Preparing);
            let completion = prepare::repository(git, task, events);
            emit_repository_finished(events, task.repository(), Phase::Preparing);
            completion
        },
    );
    let elapsed = started.elapsed();
    let prepared = completions.iter().filter(|completion| completion.prepared()).count();
    let mut updates = Vec::new();

    for completion in completions {
        match completion {
            prepare::Completion::Entry { index, entry, .. } => entries[index] = Some(entry),
            prepare::Completion::Update(task) => updates.push(task),
        }
    }

    let summary = PhaseSummary::new(prepared, elapsed);
    events.emit(Event::PhaseCompleted { phase: Phase::Preparing, summary });
    (updates, summary)
}

fn update_phase(
    git: &impl GitClient,
    tasks: &[update::Task<'_>],
    entries: &mut [Option<Entry>],
    parallelism: usize,
    events: &impl EventSink,
) -> PhaseSummary {
    if tasks.is_empty() {
        return PhaseSummary::default();
    }

    events.emit(Event::PhaseStarted { phase: Phase::Updating, total: tasks.len() });
    let started = Instant::now();
    let outcomes = workers::map_keyed(
        tasks,
        parallelism,
        |task| task.resource().to_path_buf(),
        |task| {
            emit_repository_started(events, task.repository(), Phase::Updating);
            let entry = update::repository(git, task);
            emit_repository_finished(events, task.repository(), Phase::Updating);
            (task.index(), entry)
        },
    );
    let elapsed = started.elapsed();
    let updated = outcomes
        .iter()
        .filter(|(_, entry)| matches!(entry.outcome(), Outcome::Updated { .. }))
        .count();

    for (index, entry) in outcomes {
        entries[index] = Some(entry);
    }

    let summary = PhaseSummary::new(updated, elapsed);
    events.emit(Event::PhaseCompleted { phase: Phase::Updating, summary });
    summary
}

fn emit_repository_started(
    events: &impl EventSink,
    repository: &RepositoryDefinition,
    phase: Phase,
) {
    events.emit(Event::RepositoryStarted {
        repository: repository.display_path().to_string(),
        phase,
    });
}

fn emit_repository_finished(
    events: &impl EventSink,
    repository: &RepositoryDefinition,
    phase: Phase,
) {
    events.emit(Event::RepositoryFinished {
        repository: repository.display_path().to_string(),
        phase,
    });
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use tempfile::TempDir;

    use super::execute;
    use crate::AppError;
    use crate::app::AppContext;
    use crate::git::{BranchDivergence, GitClient, GitProgressSink, GitUpdate};

    #[test]
    fn serializes_shared_git_state_without_disabling_independent_concurrency() {
        if std::thread::available_parallelism().unwrap().get() < 2 {
            return;
        }

        let root = TempDir::new().unwrap();
        let config = root.path().join("grove.toml");
        std::fs::write(
            &config,
            r#"
version = 1

[[repo]]
name = "third"
path = "third"
url = "https://example.com/third.git"

[[repo]]
name = "first"
path = "first"
url = "https://example.com/first.git"

[[repo]]
name = "second"
path = "second"
url = "https://example.com/second.git"
"#,
        )
        .unwrap();
        std::fs::create_dir(root.path().join("first")).unwrap();
        std::fs::create_dir(root.path().join("second")).unwrap();
        let ctx = AppContext::new(ConcurrentGit::default());

        let report = execute(&ctx, Some(&config), &[], false).unwrap();

        assert!(ctx.git().peak.load(Ordering::SeqCst) >= 2);
        assert_eq!(ctx.git().fetch_peak.load(Ordering::SeqCst), 1);
        assert_eq!(ctx.git().update_peak.load(Ordering::SeqCst), 1);
        assert_eq!(
            report.entries().iter().map(|entry| entry.repository()).collect::<Vec<_>>(),
            ["third", "first", "second"]
        );
        assert_eq!(report.phases().prepared().count(), 3);
    }

    #[derive(Debug, Default)]
    struct ConcurrentGit {
        active: AtomicUsize,
        peak: AtomicUsize,
        fetch_active: AtomicUsize,
        fetch_peak: AtomicUsize,
        update_active: AtomicUsize,
        update_peak: AtomicUsize,
    }

    impl ConcurrentGit {
        fn record_peak(peak: &AtomicUsize, active: usize) {
            let mut current = peak.load(Ordering::SeqCst);
            while active > current {
                match peak.compare_exchange(current, active, Ordering::SeqCst, Ordering::SeqCst) {
                    Ok(_) => break,
                    Err(actual) => current = actual,
                }
            }
        }

        fn prepare(&self) {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            Self::record_peak(&self.peak, active);

            let deadline = Instant::now() + Duration::from_millis(500);
            while self.peak.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
                std::thread::yield_now();
            }

            self.active.fetch_sub(1, Ordering::SeqCst);
        }

        fn update(&self) {
            let active = self.update_active.fetch_add(1, Ordering::SeqCst) + 1;
            Self::record_peak(&self.update_peak, active);

            let deadline = Instant::now() + Duration::from_millis(100);
            while self.update_peak.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
                std::thread::yield_now();
            }

            self.update_active.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl GitClient for ConcurrentGit {
        fn verify_available(&self) -> Result<(), AppError> {
            Ok(())
        }

        fn clone_repository(
            &self,
            _url: &str,
            _destination: &Path,
            _progress: &mut dyn GitProgressSink,
        ) -> Result<(), AppError> {
            self.prepare();
            Ok(())
        }

        fn fetch(
            &self,
            _repository: &Path,
            _progress: &mut dyn GitProgressSink,
        ) -> Result<(), AppError> {
            let active = self.fetch_active.fetch_add(1, Ordering::SeqCst) + 1;
            Self::record_peak(&self.fetch_peak, active);
            self.prepare();
            self.fetch_active.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }

        fn common_directory(&self, _repository: &Path) -> Result<PathBuf, AppError> {
            Ok(PathBuf::from("/shared/common.git"))
        }

        fn is_work_tree(&self, _repository: &Path) -> Result<bool, AppError> {
            Ok(true)
        }

        fn current_branch(&self, _repository: &Path) -> Result<Option<String>, AppError> {
            Ok(Some("main".to_string()))
        }

        fn working_tree_clean(&self, _repository: &Path) -> Result<bool, AppError> {
            Ok(true)
        }

        fn remote_url(&self, repository: &Path) -> Result<Option<String>, AppError> {
            let name = repository.file_name().unwrap().to_string_lossy();
            Ok(Some(format!("https://example.com/{name}.git")))
        }

        fn default_branch(
            &self,
            _repository: &Path,
            _configured: Option<&str>,
        ) -> Result<Option<String>, AppError> {
            Ok(Some("main".to_string()))
        }

        fn local_branch_exists(&self, _repository: &Path, _branch: &str) -> Result<bool, AppError> {
            Ok(true)
        }

        fn remote_branch_exists(
            &self,
            _repository: &Path,
            _branch: &str,
        ) -> Result<bool, AppError> {
            Ok(true)
        }

        fn branch_divergence(
            &self,
            _repository: &Path,
            _branch: &str,
        ) -> Result<Option<BranchDivergence>, AppError> {
            Ok(Some(BranchDivergence::new(0, 0)))
        }

        fn short_revision(&self, _repository: &Path, _reference: &str) -> Result<String, AppError> {
            unreachable!()
        }

        fn update_default_branch(
            &self,
            _repository: &Path,
            _branch: &str,
            _current_branch: &str,
        ) -> Result<GitUpdate, AppError> {
            self.update();
            Ok(GitUpdate::new("abc1234".to_string(), "abc1234".to_string()))
        }
    }
}
