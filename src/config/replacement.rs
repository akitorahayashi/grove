use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::addition::{Persistence, persist};
use super::discovery::locate_destination;
use super::include;
use super::lock::DirectoryLock;
use super::transition::{Repoint, Transition, sources};
use super::{ResolvedConfig, load_with_replacement, validation};
use crate::AppError;
use crate::repositories::RemovedRepository;

/// The root configuration's replacement with a candidate, settled under the
/// root configuration's directory lock: the configuration the candidate makes,
/// and what the disk must do to follow it.
pub(crate) struct Replacement {
    destination: PathBuf,
    baseline: Option<String>,
    contents: String,
    prospective: ResolvedConfig,
    transition: Transition,
    _lock: DirectoryLock,
}

impl Replacement {
    /// The candidate is evaluated as the root file itself, so the root's
    /// sibling override and includes merge into it as they do into the root.
    pub(crate) fn open(
        explicit_config: Option<&Path>,
        candidate: &Path,
        dry_run: bool,
    ) -> Result<Self, AppError> {
        let destination = locate_destination(explicit_config)?;
        let root_directory = destination
            .parent()
            .ok_or_else(|| {
                AppError::config_error(format!("{} has no parent", destination.display()))
            })?
            .to_path_buf();
        let lock = DirectoryLock::acquire(&root_directory, dry_run)?;

        // The current tree is evaluated from the bytes the final write compares
        // against, so a change in between cannot pass for the converged one.
        let baseline = match fs::read_to_string(&destination) {
            Ok(contents) => Some(contents),
            Err(err) if err.kind() == io::ErrorKind::NotFound && explicit_config.is_some() => None,
            Err(err) => {
                return Err(super::source_error(format!("{}: {err}", destination.display()), err));
            }
        };
        let current = match &baseline {
            Some(contents) => load_with_replacement(&destination, &destination, contents)?,
            None => ResolvedConfig::new(destination.clone(), Vec::new(), Vec::new(), Vec::new()),
        };

        let label = candidate.display().to_string();
        let contents = fs::read_to_string(candidate)
            .map_err(|err| super::source_error(format!("{label}: {err}"), err))?;
        let (prospective, sources) =
            include::load_tree_with_replacement(&destination, &destination, &contents)
                .and_then(|tree| {
                    let sources = sources(&tree, None)?;
                    Ok((validation::resolve(tree)?, sources))
                })
                .map_err(|error| {
                    super::source_error(
                        format!(
                            "{label}: replacing {} with it would leave an invalid configuration: \
                             {error}",
                            destination.display()
                        ),
                        error,
                    )
                })?;
        let transition =
            Transition::replacing(&current, &prospective, &root_directory, &sources, &label)?;

        Ok(Self { destination, baseline, contents, prospective, transition, _lock: lock })
    }

    pub(crate) fn destination(&self) -> &Path {
        &self.destination
    }

    /// The configuration the candidate makes, its override and includes merged.
    pub(crate) fn prospective(&self) -> &ResolvedConfig {
        &self.prospective
    }

    /// The removed declarations to converge against the prospective
    /// configuration: those leaving it, then its own new ones.
    pub(crate) fn removals(&self) -> impl Iterator<Item = &RemovedRepository> {
        self.transition.leaving.iter().chain(&self.transition.tombstones)
    }

    pub(crate) fn repoints(&self) -> &[Repoint] {
        &self.transition.repoints
    }

    /// Whether the destination already holds the candidate's bytes.
    pub(crate) fn is_current(&self) -> bool {
        self.baseline.as_deref() == Some(self.contents.as_str())
    }

    /// Fails without writing when the destination no longer holds the bytes
    /// the current configuration was read from.
    pub(crate) fn write(&self) -> Result<Persistence, AppError> {
        persist(&self.destination, self.baseline.as_deref(), self.contents.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::Replacement;
    use crate::AppError;

    const ALPHA: &str = "git@example.com:company/alpha.git";

    struct Grove {
        _root: TempDir,
        directory: PathBuf,
        candidate: PathBuf,
    }

    impl Grove {
        fn new(candidate: &str) -> Self {
            let root = TempDir::new().unwrap();
            let directory = root.path().canonicalize().unwrap().join("workspace");
            fs::create_dir(&directory).unwrap();
            let candidate_path = root.path().join("candidate.toml");
            fs::write(&candidate_path, candidate).unwrap();
            Self { _root: root, directory, candidate: candidate_path }
        }

        fn open(&self, config: Option<&str>) -> Result<Replacement, AppError> {
            let config = config.map(|relative| self.directory.join(relative));
            Replacement::open(config.as_deref(), &self.candidate, true)
        }
    }

    #[test]
    fn an_explicit_absent_destination_starts_from_an_empty_configuration() {
        let grove = Grove::new(&format!("version = 2\n[repos]\nalpha = \"{ALPHA}\"\n"));

        let replacement = grove.open(Some("grove.toml")).unwrap();

        assert_eq!(replacement.destination(), grove.directory.join("grove.toml"));
        assert_eq!(replacement.prospective().repositories().len(), 1);
        assert_eq!(replacement.removals().count(), 0);
        assert!(replacement.repoints().is_empty());
        assert!(!replacement.is_current());
    }

    #[test]
    fn an_absent_destination_needs_an_existing_directory() {
        let grove = Grove::new("version = 2\n");

        assert!(grove.open(Some("missing/grove.toml")).is_err());
    }

    #[test]
    fn an_existing_destination_must_be_valid() {
        let grove = Grove::new("version = 2\n");
        fs::write(grove.directory.join("grove.toml"), "version = 3\n").unwrap();

        let error = grove.open(Some("grove.toml")).err().unwrap();

        assert!(error.to_string().contains("unsupported config version 3"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn a_broken_link_at_the_destination_is_never_taken_for_an_absent_file() {
        let grove = Grove::new("version = 2\n");
        std::os::unix::fs::symlink("missing.toml", grove.directory.join("grove.toml")).unwrap();

        assert!(grove.open(Some("grove.toml")).is_err());
    }

    #[test]
    fn an_invalid_candidate_names_itself() {
        let grove = Grove::new("[repos]\n");
        fs::write(grove.directory.join("grove.toml"), "version = 2\n").unwrap();

        let error = grove.open(Some("grove.toml")).err().unwrap();

        let message = error.to_string();
        assert!(message.contains(&grove.candidate.display().to_string()), "{message}");
        assert!(message.contains("would leave an invalid configuration"), "{message}");
    }

    #[test]
    fn an_identical_candidate_is_current() {
        let contents = format!("version = 2\n[repos]\nalpha = \"{ALPHA}\"\n");
        let grove = Grove::new(&contents);
        fs::write(grove.directory.join("grove.toml"), &contents).unwrap();

        let replacement = grove.open(Some("grove.toml")).unwrap();

        assert!(replacement.is_current());
    }
}
