//! Single-repository clone through the local cache, independent of any
//! configuration file.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use crate::AppError;
use crate::app::AppContext;
use crate::cache::{Outcome as CacheOutcome, Shorthand};
use crate::git::{
    CacheEntry, CloneCacheDecision, CloneCommand, CloneInvocation, NoopGitProgressSink,
    RepositoryProbe,
};
use crate::repositories::RemoteUrl;

#[derive(Debug)]
pub(crate) enum CommandCache {
    Used(CacheOutcome),
    Bypassed(&'static str),
    Delegated,
    Unavailable(String),
}

#[derive(Debug)]
pub(crate) struct CommandReport {
    status: ExitStatus,
    cache: CommandCache,
    quiet: bool,
    resolved: Option<(Shorthand, RemoteUrl)>,
}

impl CommandReport {
    /// The cache shorthand the repository operand named, and the URL it
    /// resolved to.
    pub(crate) fn resolved(&self) -> Option<(&Shorthand, &RemoteUrl)> {
        self.resolved.as_ref().map(|(shorthand, url)| (shorthand, url))
    }

    pub(crate) fn status(&self) -> ExitStatus {
        self.status
    }

    pub(crate) fn cache(&self) -> &CommandCache {
        &self.cache
    }

    pub(crate) fn quiet(&self) -> bool {
        self.quiet
    }
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

pub fn execute<G: CacheEntry + RepositoryProbe, Z>(
    ctx: &AppContext<G, Z>,
    url: &str,
    destination: Option<PathBuf>,
) -> Result<Report, AppError> {
    ctx.git().verify_available()?;
    let store = ctx.cache()?;
    let url = RemoteUrl::new(url)?;
    let destination = resolve_destination(&url, destination)?;

    let started = Instant::now();
    let cache = store.place(ctx.git(), &url, &destination, None, None, &mut NoopGitProgressSink)?;
    Ok(Report { destination, url: url.to_string(), cache, elapsed: started.elapsed() })
}

pub(crate) fn execute_command<G: CacheEntry + CloneCommand + RepositoryProbe, Z>(
    ctx: &AppContext<G, Z>,
    arguments: Vec<OsString>,
) -> Result<CommandReport, AppError> {
    let mut invocation = CloneInvocation::new(arguments);
    let resolved = match invocation.repository().and_then(cache_shorthand) {
        Some(shorthand) => {
            let url = ctx.cache()?.resolve(ctx.git(), &shorthand)?;
            invocation = invocation.with_repository(&url);
            Some((shorthand, url))
        }
        None => None,
    };
    let (reference, cache) = match invocation.cache() {
        CloneCacheDecision::Eligible { url, insertion_index } => {
            let mut progress = NoopGitProgressSink;
            match ctx
                .cache()
                .and_then(|store| store.prepare_clone_reference(ctx.git(), url, &mut progress))
            {
                Ok((reference, outcome)) => {
                    (Some((reference, *insertion_index)), CommandCache::Used(outcome))
                }
                Err(error) => (None, CommandCache::Unavailable(error.to_string())),
            }
        }
        CloneCacheDecision::Bypassed(reason) => (None, CommandCache::Bypassed(reason)),
        CloneCacheDecision::Delegated => (None, CommandCache::Delegated),
    };

    let status = ctx.git().clone_command(
        &invocation,
        reference.as_ref().map(|(path, index)| (path.as_path(), *index)),
    )?;
    Ok(CommandReport { status, cache, quiet: invocation.quiet(), resolved })
}

/// Git reads an operand as a local repository when it, or it with `.git` or
/// `.bundle` appended, exists; such an operand stays Git's so a local clone
/// keeps its meaning.
fn cache_shorthand(operand: &str) -> Option<Shorthand> {
    let shorthand = Shorthand::parse(operand)?;
    let local = ["", ".git", ".bundle"]
        .into_iter()
        .any(|suffix| Path::new(&format!("{operand}{suffix}")).exists());
    (!local).then_some(shorthand)
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
