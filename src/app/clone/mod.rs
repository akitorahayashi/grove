//! Single-repository clone through the local cache, independent of any
//! configuration file.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::AppError;
use crate::app::AppContext;
use crate::app::cache::{CacheOutcome, CacheStore};
use crate::app::events::{DiscardEvents, Event, EventSink};
use crate::git::{GitClient, GitProgress, GitProgressSink};
use crate::repositories::RemoteUrl;

pub use crate::app::events::PhaseSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Cloning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    destination: PathBuf,
    url: String,
    cache: CacheOutcome,
    elapsed: Duration,
}

impl Report {
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn cache(&self) -> CacheOutcome {
        self.cache
    }

    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }
}

pub fn execute(
    ctx: &AppContext<impl GitClient, impl crate::zoxide::ZoxideClient>,
    url: &str,
    destination: Option<PathBuf>,
) -> Result<Report, AppError> {
    execute_with_events(ctx, url, destination, &DiscardEvents)
}

pub(crate) fn execute_with_events(
    ctx: &AppContext<impl GitClient, impl crate::zoxide::ZoxideClient>,
    url: &str,
    destination: Option<PathBuf>,
    events: &impl EventSink<Phase>,
) -> Result<Report, AppError> {
    ctx.git().verify_available()?;
    let cache = CacheStore::from_env()?;
    let url = RemoteUrl::new(url)?;
    let destination = resolve_destination(&url, destination)?;
    let name = display_name(&destination);
    let started = Instant::now();

    events.emit(Event::PhaseStarted { phase: Phase::Cloning, total: 1 })?;
    events.emit(Event::RepositoryStarted { repository: name.clone(), phase: Phase::Cloning })?;
    let mut progress = NamedProgress { name: name.clone(), events };
    let outcome = cache.place(ctx.git(), &url, &destination, None, None, &mut progress);
    events.emit(Event::RepositoryFinished { repository: name, phase: Phase::Cloning })?;

    let cache = match outcome {
        Ok(cache) => cache,
        Err(err) => {
            events.emit(Event::PhaseFailed { phase: Phase::Cloning })?;
            return Err(err);
        }
    };

    let elapsed = started.elapsed();
    events.emit(Event::PhaseCompleted {
        phase: Phase::Cloning,
        summary: PhaseSummary::new(1, elapsed),
    })?;
    Ok(Report { destination, url: url.to_string(), cache, elapsed })
}

fn resolve_destination(url: &RemoteUrl, destination: Option<PathBuf>) -> Result<PathBuf, AppError> {
    let relative = match destination {
        Some(destination) => destination,
        None => PathBuf::from(default_destination_name(url.as_process_argument())?),
    };
    Ok(std::env::current_dir()?.join(relative))
}

fn default_destination_name(url: &str) -> Result<String, AppError> {
    let trimmed = url.trim_end_matches('/');
    let tail = trimmed.rsplit(['/', ':']).next().unwrap_or_default();
    let name = tail.strip_suffix(".git").unwrap_or(tail);
    if name.is_empty() {
        Err(AppError::config_error(
            "cannot infer a destination directory from the URL; specify one explicitly",
        ))
    } else {
        Ok(name.to_string())
    }
}

fn display_name(destination: &Path) -> String {
    destination
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| destination.display().to_string())
}

struct NamedProgress<'a, P> {
    name: String,
    events: &'a dyn EventSink<P>,
}

impl<P> GitProgressSink for NamedProgress<'_, P> {
    fn progress(&mut self, progress: GitProgress) -> Result<(), AppError> {
        self.events.emit(Event::GitProgress { repository: self.name.clone(), progress })
    }
}
