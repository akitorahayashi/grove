//! Single-repository clone through the local cache, independent of any
//! configuration file.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::AppError;
use crate::app::AppContext;
use crate::cache::Outcome as CacheOutcome;
use crate::git::{GitClient, GitProgress, GitProgressSink};
use crate::phases::{DiscardEvents, Event, EventSink};
use crate::repositories::RemoteUrl;

pub use crate::phases::Summary as PhaseSummary;

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
    let cache = ctx.cache();
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
    // Derive the name from the path only. Query and fragment components can
    // carry credentials (`?access_token=...`); keeping them would place the
    // secret in the filesystem path and print it unredacted.
    let path = url.split(['?', '#']).next().unwrap_or_default();
    let trimmed = path.trim_end_matches('/');
    let tail = trimmed.rsplit(['/', ':']).next().unwrap_or_default();
    let name = tail.strip_suffix(".git").unwrap_or(tail);
    if name.is_empty() || name == "." || name == ".." {
        Err(AppError::invalid_arguments(
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

#[cfg(test)]
mod tests {
    use super::default_destination_name;

    #[test]
    fn infers_name_from_path_tail() {
        assert_eq!(default_destination_name("https://example.com/org/repo.git").unwrap(), "repo");
        assert_eq!(default_destination_name("git@example.com:org/repo.git").unwrap(), "repo");
        assert_eq!(default_destination_name("https://example.com/org/repo/").unwrap(), "repo");
    }

    #[test]
    fn drops_query_and_fragment_so_credentials_never_reach_the_path() {
        assert_eq!(
            default_destination_name("https://example.com/repo.git?access_token=secret").unwrap(),
            "repo"
        );
        assert_eq!(
            default_destination_name("https://example.com/repo.git#fragment").unwrap(),
            "repo"
        );
    }

    #[test]
    fn rejects_names_that_are_not_a_usable_directory() {
        for url in ["https://example.com/.git", "https://example.com/..", "https://example.com/."] {
            assert!(default_destination_name(url).is_err(), "{url} should be rejected");
        }
    }
}
