//! Local clone cache.
//!
//! A cache entry is a bare, single-branch repository under the user cache
//! directory, keyed by the verbatim remote URL. Placement clones the real
//! remote while borrowing objects from the entry (`--reference --dissociate`),
//! so the placed clone is self-contained and correct even when the entry is
//! stale or narrow — the entry only reduces network transfer.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::AppError;
use crate::git::{GitClient, GitProgressSink};
use crate::repositories::{
    BranchName, RemoteUrl, ResolutionError, redact_urls_for_display, resolve_operational_path,
};

/// What happened to the cache entry backing a placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// No entry existed; a new one was created.
    Miss,
    /// The entry existed and its tracked branch was refreshed.
    Hit,
    /// The entry was unusable and rebuilt from the remote.
    Rebuilt,
    /// The entry was pointed at a different branch, then refreshed.
    Retargeted,
}

/// One cache entry as surfaced by `gv cache list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntryInfo {
    url: String,
    size_bytes: u64,
    modified: Option<SystemTime>,
}

impl EntryInfo {
    pub(crate) fn url(&self) -> &str {
        &self.url
    }

    pub(crate) fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    pub(crate) fn modified(&self) -> Option<SystemTime> {
        self.modified
    }
}

/// The local clone cache rooted at a single directory.
pub(crate) struct Store {
    root: PathBuf,
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl Store {
    /// Resolve the cache root from the environment: `${XDG_CACHE_HOME}/grove`,
    /// falling back to `${HOME}/.cache/grove`. Both being unset is an error
    /// rather than a silent fallback to some other location.
    pub(crate) fn from_env() -> Result<Self, AppError> {
        let root = cache_root_from_env()?;
        Ok(Self { root, locks: Mutex::new(HashMap::new()) })
    }

    #[cfg(test)]
    pub(crate) fn with_root(root: PathBuf) -> Self {
        Self { root, locks: Mutex::new(HashMap::new()) }
    }

    /// Clone `url` into `destination`, ensuring and reusing a cache entry.
    ///
    /// When `grove_root` is `Some`, `destination` is validated to stay inside
    /// that root (the sync contract); when `None`, only an existing non-empty
    /// destination is rejected (the `gv clone` contract).
    pub(crate) fn place(
        &self,
        git: &impl GitClient,
        url: &RemoteUrl,
        destination: &Path,
        grove_root: Option<&Path>,
        branch: Option<&BranchName>,
        progress: &mut dyn GitProgressSink,
    ) -> Result<Outcome, AppError> {
        guard_destination(destination, grove_root)?;

        let key = url.as_process_argument();
        let lock = self.entry_lock(key);
        let _guard = lock.lock().expect("cache entry lock poisoned");

        let container = self.root.join(entry_directory_name(key));
        let bare = container.join("git");
        let wanted = branch.map(BranchName::as_str);

        let outcome = self.ensure_entry(git, url, &container, &bare, wanted, progress)?;
        // Fetches land beneath `<container>/git`, which never advances the outer
        // container's own mtime. Record the last successful cache operation
        // explicitly so `cache list` reports a truthful age rather than the
        // container's creation time.
        touch_updated(&container)?;
        git.clone_with_reference(url, destination, &bare, progress)?;
        Ok(outcome)
    }

    /// Seed a cache entry for `url` from an already-present local clone,
    /// without placing any destination. The remote's default branch is tracked
    /// (as with `gv clone`), and objects are borrowed from `source` and
    /// dissociated, so an existing repository populates the cache without a full
    /// re-download. An entry that already exists is left untouched. Returns
    /// whether a new entry was created.
    pub(crate) fn seed_from_local(
        &self,
        git: &impl GitClient,
        url: &RemoteUrl,
        source: &Path,
        progress: &mut dyn GitProgressSink,
    ) -> Result<bool, AppError> {
        let key = url.as_process_argument();
        let lock = self.entry_lock(key);
        let _guard = lock.lock().expect("cache entry lock poisoned");

        let container = self.root.join(entry_directory_name(key));
        if container.exists() {
            return Ok(false);
        }

        self.build_entry(git, url, &container, None, Some(source), progress)?;
        touch_updated(&container)?;
        Ok(true)
    }

    /// Whether a cache entry already exists for `url`. A cheap pre-check so
    /// callers can skip resolving a seed source for an already-cached URL; the
    /// authoritative check happens under the entry lock in `seed_from_local`.
    pub(crate) fn is_cached(&self, url: &RemoteUrl) -> bool {
        self.root.join(entry_directory_name(url.as_process_argument())).exists()
    }

    /// Enumerate cache entries for reporting.
    pub(crate) fn list(&self) -> Result<Vec<EntryInfo>, AppError> {
        let mut infos = Vec::new();
        if !self.root.exists() {
            return Ok(infos);
        }
        for entry in fs::read_dir(&self.root)? {
            let container = entry?.path();
            if !container.is_dir() {
                continue;
            }
            let Some(url) = read_metadata(&container, "url")? else {
                continue;
            };
            infos.push(EntryInfo {
                url,
                size_bytes: directory_size(&container)?,
                modified: fs::metadata(container.join("updated"))
                    .and_then(|meta| meta.modified())
                    .ok(),
            });
        }
        infos.sort_by(|left, right| left.url.cmp(&right.url));
        Ok(infos)
    }

    /// Remove every cache entry, returning how many were removed.
    pub(crate) fn clean_all(&self) -> Result<usize, AppError> {
        let mut removed = 0;
        if !self.root.exists() {
            return Ok(0);
        }
        for entry in fs::read_dir(&self.root)? {
            let container = entry?.path();
            if !container.is_dir() {
                continue;
            }
            let counts = read_metadata(&container, "url")?.is_some();
            fs::remove_dir_all(&container)?;
            if counts {
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Remove the cache entry for a single URL, if present.
    pub(crate) fn remove(&self, url: &RemoteUrl) -> Result<bool, AppError> {
        let container = self.root.join(entry_directory_name(url.as_process_argument()));
        if container.exists() {
            fs::remove_dir_all(&container)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn ensure_entry(
        &self,
        git: &impl GitClient,
        url: &RemoteUrl,
        container: &Path,
        bare: &Path,
        wanted: Option<&str>,
        progress: &mut dyn GitProgressSink,
    ) -> Result<Outcome, AppError> {
        if !container.exists() {
            self.build_entry(git, url, container, wanted, None, progress)?;
            return Ok(Outcome::Miss);
        }

        match read_metadata(container, "url")? {
            Some(recorded) if recorded == url.as_process_argument() => {}
            Some(_) => {
                return Err(AppError::cache_state(format!(
                    "cache entry '{}' records a different URL",
                    container.display()
                )));
            }
            None => {
                self.build_entry(git, url, container, wanted, None, progress)?;
                return Ok(Outcome::Rebuilt);
            }
        }

        if !git.cache_verify(bare)? {
            self.build_entry(git, url, container, wanted, None, progress)?;
            return Ok(Outcome::Rebuilt);
        }

        if let Some(wanted) = wanted
            && read_metadata(container, "branch")?.as_deref() != Some(wanted)
        {
            git.cache_retarget(bare, wanted, progress)?;
            write_branch(container, wanted)?;
            return Ok(Outcome::Retargeted);
        }

        git.cache_update(bare, progress)?;
        Ok(Outcome::Hit)
    }

    fn build_entry(
        &self,
        git: &impl GitClient,
        url: &RemoteUrl,
        container: &Path,
        wanted: Option<&str>,
        reference: Option<&Path>,
        progress: &mut dyn GitProgressSink,
    ) -> Result<(), AppError> {
        let temporary = temporary_path(container);
        if temporary.exists() {
            fs::remove_dir_all(&temporary)?;
        }

        let tracked = git.cache_create(url, &temporary.join("git"), wanted, reference, progress)?;
        fs::write(temporary.join("url"), url.as_process_argument())?;
        write_branch(&temporary, &tracked)?;

        if container.exists() {
            fs::remove_dir_all(container)?;
        }
        fs::rename(&temporary, container)?;
        Ok(())
    }

    fn entry_lock(&self, key: &str) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().expect("cache lock map poisoned");
        Arc::clone(locks.entry(key.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))))
    }
}

fn guard_destination(destination: &Path, grove_root: Option<&Path>) -> Result<(), AppError> {
    if let Some(root) = grove_root {
        match resolve_operational_path(destination, root) {
            Ok(resolved) if resolved == destination => {}
            Ok(resolved) => {
                return Err(AppError::cache_state(format!(
                    "clone destination changed after validation: '{}' resolves to '{}'",
                    destination.display(),
                    resolved.display()
                )));
            }
            Err(ResolutionError::OutsideRoot) => {
                return Err(AppError::cache_state(format!(
                    "clone destination '{}' leaves the grove root",
                    destination.display()
                )));
            }
            Err(ResolutionError::Io(err)) => return Err(err.into()),
        }
    }

    if destination.is_dir() {
        if fs::read_dir(destination)?.next().is_some() {
            return Err(AppError::cache_state(format!(
                "destination '{}' already exists and is not empty",
                destination.display()
            )));
        }
    } else if destination.exists() {
        return Err(AppError::cache_state(format!(
            "destination '{}' already exists",
            destination.display()
        )));
    }
    Ok(())
}

fn cache_root_from_env() -> Result<PathBuf, AppError> {
    if let Some(value) = std::env::var_os("XDG_CACHE_HOME")
        && !value.is_empty()
    {
        return Ok(PathBuf::from(value).join("grove"));
    }
    if let Some(home) = std::env::var_os("HOME")
        && !home.is_empty()
    {
        return Ok(PathBuf::from(home).join(".cache").join("grove"));
    }
    Err(AppError::cache_state("cannot determine the cache directory: set XDG_CACHE_HOME or HOME"))
}

fn entry_directory_name(url: &str) -> String {
    // The hash keys on the verbatim URL, but the human-readable slug is built
    // from the redacted URL so credentials never land in a directory name.
    let slug = slug(&redact_urls_for_display(url));
    let hash = fnv1a(url.as_bytes());
    if slug.is_empty() { format!("{hash:016x}") } else { format!("{slug}-{hash:016x}") }
}

fn slug(url: &str) -> String {
    let mut slug = String::new();
    let mut pending_separator = false;
    for character in url.chars() {
        if character.is_ascii_alphanumeric() {
            if pending_separator {
                slug.push('-');
                pending_separator = false;
            }
            slug.push(character.to_ascii_lowercase());
        } else if !slug.is_empty() {
            pending_separator = true;
        }
    }
    slug.chars().take(48).collect::<String>().trim_end_matches('-').to_string()
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn temporary_path(container: &Path) -> PathBuf {
    let mut name = container.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    container.with_file_name(name)
}

fn read_metadata(container: &Path, name: &str) -> Result<Option<String>, AppError> {
    match fs::read_to_string(container.join(name)) {
        Ok(value) => Ok(Some(value)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn write_branch(container: &Path, branch: &str) -> Result<(), AppError> {
    fs::write(container.join("branch"), branch)?;
    Ok(())
}

/// Stamp the container's last successful cache operation. The marker's mtime,
/// not the container directory's, is what `cache list` reports as the age.
fn touch_updated(container: &Path) -> Result<(), AppError> {
    fs::write(container.join("updated"), [])?;
    Ok(())
}

fn directory_size(path: &Path) -> Result<u64, AppError> {
    let mut total = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            total += directory_size(&entry.path())?;
        } else {
            total += metadata.len();
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use tempfile::TempDir;

    use super::{Outcome, Store};
    use crate::AppError;
    use crate::git::{CommandGitClient, NoopGitProgressSink};
    use crate::repositories::{BranchName, RemoteUrl};

    fn run_git(directory: &Path, args: &[&str]) {
        let output =
            Command::new("git").current_dir(directory).args(args).output().expect("run git");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn make_remote(base: &Path, feature: bool) -> PathBuf {
        fs::create_dir_all(base).unwrap();
        let remote = base.join("remote.git");
        let seed = base.join("seed");
        run_git(base, &["init", "--bare", "--initial-branch=main", remote.to_str().unwrap()]);
        run_git(base, &["init", "-b", "main", seed.to_str().unwrap()]);
        fs::write(seed.join("README.md"), "initial\n").unwrap();
        run_git(&seed, &["add", "README.md"]);
        run_git(&seed, &["-c", "user.name=T", "-c", "user.email=t@e.x", "commit", "-m", "initial"]);
        run_git(&seed, &["remote", "add", "origin", remote.to_str().unwrap()]);
        run_git(&seed, &["push", "-u", "origin", "main"]);
        if feature {
            run_git(&seed, &["switch", "-c", "feature"]);
            run_git(&seed, &["push", "-u", "origin", "feature"]);
        }
        remote
    }

    fn url_of(remote: &Path) -> RemoteUrl {
        RemoteUrl::new(remote.to_str().unwrap()).unwrap()
    }

    fn single_entry(cache_root: &Path) -> PathBuf {
        let mut entries = fs::read_dir(cache_root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 1, "expected exactly one cache entry");
        entries.remove(0)
    }

    #[test]
    fn miss_creates_entry_then_hit_reuses_it() {
        let tmp = TempDir::new().unwrap();
        let remote = make_remote(&tmp.path().join("origin"), false);
        let store = Store::with_root(tmp.path().join("cache"));
        let git = CommandGitClient::default();
        let url = url_of(&remote);

        let first = store
            .place(&git, &url, &tmp.path().join("a"), None, None, &mut NoopGitProgressSink)
            .unwrap();
        assert_eq!(first, Outcome::Miss);
        assert!(tmp.path().join("a").join(".git").exists());

        let second = store
            .place(&git, &url, &tmp.path().join("b"), None, None, &mut NoopGitProgressSink)
            .unwrap();
        assert_eq!(second, Outcome::Hit);
        assert!(tmp.path().join("b").join(".git").exists());
    }

    #[test]
    fn corrupt_entry_is_rebuilt() {
        let tmp = TempDir::new().unwrap();
        let remote = make_remote(&tmp.path().join("origin"), false);
        let cache_root = tmp.path().join("cache");
        let store = Store::with_root(cache_root.clone());
        let git = CommandGitClient::default();
        let url = url_of(&remote);

        store
            .place(&git, &url, &tmp.path().join("a"), None, None, &mut NoopGitProgressSink)
            .unwrap();
        fs::remove_dir_all(single_entry(&cache_root).join("git")).unwrap();

        let outcome = store
            .place(&git, &url, &tmp.path().join("b"), None, None, &mut NoopGitProgressSink)
            .unwrap();
        assert_eq!(outcome, Outcome::Rebuilt);
        assert!(tmp.path().join("b").join(".git").exists());
    }

    #[test]
    fn wanted_branch_change_retargets_entry() {
        let tmp = TempDir::new().unwrap();
        let remote = make_remote(&tmp.path().join("origin"), true);
        let store = Store::with_root(tmp.path().join("cache"));
        let git = CommandGitClient::default();
        let url = url_of(&remote);
        let feature = BranchName::new("feature").unwrap();

        store
            .place(&git, &url, &tmp.path().join("a"), None, None, &mut NoopGitProgressSink)
            .unwrap();

        let outcome = store
            .place(
                &git,
                &url,
                &tmp.path().join("b"),
                None,
                Some(&feature),
                &mut NoopGitProgressSink,
            )
            .unwrap();
        assert_eq!(outcome, Outcome::Retargeted);
    }

    #[test]
    fn mismatched_url_metadata_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let remote = make_remote(&tmp.path().join("origin"), false);
        let cache_root = tmp.path().join("cache");
        let store = Store::with_root(cache_root.clone());
        let git = CommandGitClient::default();
        let url = url_of(&remote);

        store
            .place(&git, &url, &tmp.path().join("a"), None, None, &mut NoopGitProgressSink)
            .unwrap();
        fs::write(single_entry(&cache_root).join("url"), "https://example.com/other.git").unwrap();

        let error = store
            .place(&git, &url, &tmp.path().join("b"), None, None, &mut NoopGitProgressSink)
            .unwrap_err();
        assert!(error.to_string().contains("records a different URL"));
    }

    #[test]
    fn update_failure_surfaces_as_a_non_internal_error() {
        let tmp = TempDir::new().unwrap();
        let remote = make_remote(&tmp.path().join("origin"), false);
        let store = Store::with_root(tmp.path().join("cache"));
        let git = CommandGitClient::default();
        let url = url_of(&remote);

        store
            .place(&git, &url, &tmp.path().join("a"), None, None, &mut NoopGitProgressSink)
            .unwrap();
        fs::remove_dir_all(&remote).unwrap();

        let error = store
            .place(&git, &url, &tmp.path().join("b"), None, None, &mut NoopGitProgressSink)
            .unwrap_err();
        assert!(!matches!(error, AppError::Internal(_)));
    }

    #[test]
    fn hit_advances_cached_branch_after_remote_moves() {
        let tmp = TempDir::new().unwrap();
        let origin = tmp.path().join("origin");
        let remote = make_remote(&origin, false);
        let cache_root = tmp.path().join("cache");
        let store = Store::with_root(cache_root.clone());
        let git = CommandGitClient::default();
        let url = url_of(&remote);

        store
            .place(&git, &url, &tmp.path().join("a"), None, None, &mut NoopGitProgressSink)
            .unwrap();

        let seed = origin.join("seed");
        fs::write(seed.join("second.txt"), "second\n").unwrap();
        run_git(&seed, &["add", "second.txt"]);
        run_git(&seed, &["-c", "user.name=T", "-c", "user.email=t@e.x", "commit", "-m", "second"]);
        run_git(&seed, &["push", "origin", "main"]);

        let outcome = store
            .place(&git, &url, &tmp.path().join("b"), None, None, &mut NoopGitProgressSink)
            .unwrap();
        assert_eq!(outcome, Outcome::Hit);

        let bare = single_entry(&cache_root).join("git");
        assert_eq!(
            git_rev(&bare, "refs/heads/main"),
            git_rev(&seed, "main"),
            "a cache hit must fast-forward the tracked branch to the new remote tip",
        );
    }

    #[test]
    fn list_reports_the_time_of_the_last_placement() {
        let tmp = TempDir::new().unwrap();
        let remote = make_remote(&tmp.path().join("origin"), false);
        let cache_root = tmp.path().join("cache");
        let store = Store::with_root(cache_root);
        let git = CommandGitClient::default();
        let url = url_of(&remote);

        store
            .place(&git, &url, &tmp.path().join("a"), None, None, &mut NoopGitProgressSink)
            .unwrap();
        let created =
            store.list().unwrap()[0].modified().expect("placement records an update time");

        std::thread::sleep(std::time::Duration::from_millis(50));

        store
            .place(&git, &url, &tmp.path().join("b"), None, None, &mut NoopGitProgressSink)
            .unwrap();
        let refreshed =
            store.list().unwrap()[0].modified().expect("placement records an update time");

        assert!(
            refreshed > created,
            "a later placement must advance the reported update time, not report entry creation",
        );
    }

    #[test]
    fn seed_from_local_populates_cache_from_an_existing_clone() {
        let tmp = TempDir::new().unwrap();
        let remote = make_remote(&tmp.path().join("origin"), false);
        let cache_root = tmp.path().join("cache");
        let store = Store::with_root(cache_root.clone());
        let git = CommandGitClient::default();
        let url = url_of(&remote);

        // A clone the user already has on disk, with no cache entry yet.
        let existing = tmp.path().join("existing");
        run_git(tmp.path(), &["clone", remote.to_str().unwrap(), existing.to_str().unwrap()]);

        let seeded = store
            .seed_from_local(&git, &url, &existing.join(".git"), &mut NoopGitProgressSink)
            .unwrap();
        assert!(seeded, "an uncached repository is seeded");

        // The entry tracks the remote's default branch at its tip.
        let bare = single_entry(&cache_root).join("git");
        assert_eq!(git_rev(&bare, "refs/heads/main"), git_rev(&remote, "refs/heads/main"));

        // A repository that is already cached is left untouched.
        let seeded_again = store
            .seed_from_local(&git, &url, &existing.join(".git"), &mut NoopGitProgressSink)
            .unwrap();
        assert!(!seeded_again, "an already-cached repository is not re-seeded");

        // The entry is self-contained: it still resolves its objects after the
        // source it borrowed from is deleted.
        fs::remove_dir_all(&existing).unwrap();
        run_git(&bare, &["fsck", "--connectivity-only"]);
    }

    fn git_rev(directory: &Path, reference: &str) -> String {
        let output = Command::new("git")
            .current_dir(directory)
            .args(["rev-parse", reference])
            .output()
            .expect("run git rev-parse");
        assert!(output.status.success(), "git rev-parse {reference} failed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }
}
