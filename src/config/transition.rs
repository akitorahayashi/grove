use std::io;
use std::path::{Path, PathBuf};

use super::ResolvedConfig;
use super::include::{LoadedConfigTree, resolve_sibling_override_path, sibling_override_path};
use crate::AppError;
use crate::repositories::{
    RemoteUrl, RemovedRepository, RepositoryDefinition, RepositoryName, ResolutionError,
    normalize_lexically, resolve_entry_path,
};

/// The change that makes declarations leave, as its messages name it.
#[derive(Clone, Copy)]
pub(super) enum Change {
    Retirement,
    Replacement,
}

impl Change {
    fn acting(self) -> &'static str {
        match self {
            Self::Retirement => "retiring",
            Self::Replacement => "removing",
        }
    }

    fn acted(self) -> &'static str {
        match self {
            Self::Retirement => "retired",
            Self::Replacement => "removed",
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Declared<'a> {
    Live(&'a RepositoryDefinition),
    Removed(&'a RemovedRepository),
}

impl<'a> Declared<'a> {
    fn url(self) -> &'a RemoteUrl {
        match self {
            Self::Live(repository) => repository.url(),
            Self::Removed(removed) => removed.url(),
        }
    }

    fn path(self) -> &'a Path {
        match self {
            Self::Live(repository) => repository.path(),
            Self::Removed(removed) => removed.path(),
        }
    }

    fn display_path(self) -> &'a str {
        match self {
            Self::Live(repository) => repository.display_path(),
            Self::Removed(removed) => removed.display_path(),
        }
    }
}

/// A path a configuration declares, with what it declares there: `route` is
/// the path as written and `location` where it leads, both spelled as
/// `on_disk` spells them.
pub(super) struct Declaration<'a> {
    pub(super) route: PathBuf,
    pub(super) location: PathBuf,
    pub(super) declared: Declared<'a>,
}

impl<'a> Declaration<'a> {
    pub(super) fn url(&self) -> &'a RemoteUrl {
        self.declared.url()
    }
}

pub(super) fn declarations<'a>(
    config: &'a ResolvedConfig,
    root: &Path,
) -> Result<Vec<Declaration<'a>>, AppError> {
    config
        .repositories()
        .iter()
        .map(Declared::Live)
        .chain(config.removed().iter().map(Declared::Removed))
        .map(|declared| {
            Ok(Declaration {
                route: on_disk(&root.join(declared.display_path()))?,
                location: on_disk(declared.path())?,
                declared,
            })
        })
        .collect()
}

/// The files a configuration is read from, both through the paths that name
/// them and where those paths lead: every loaded base file and every override
/// merged into one, apart from the override of `retired_base`.
pub(super) fn sources(
    tree: &LoadedConfigTree,
    retired_base: Option<&Path>,
) -> Result<Vec<PathBuf>, AppError> {
    let mut sources = Vec::new();
    for file in &tree.files {
        sources.push(file.path.clone());
        for include in &file.raw.include {
            sources.push(on_disk(&normalize_lexically(&file.directory.join(include)))?);
        }
        if retired_base != Some(file.path.as_path())
            && let Some(resolved) = resolve_sibling_override_path(&file.path)?
        {
            sources.push(on_disk(&sibling_override_path(&file.path))?);
            sources.push(resolved);
        }
    }
    Ok(sources)
}

/// A live repository's path as a removed declaration's path resolves, so a
/// symbolic link there is assessed as the link rather than as its target.
pub(super) fn entry_path(
    repository: &RepositoryDefinition,
    change: Change,
    label: &str,
) -> Result<PathBuf, AppError> {
    if repository.path() == repository.root() {
        return Err(AppError::config_error(format!(
            "{label}: repository '{}' is the grove root, which cannot be {}",
            repository.name(),
            change.acted()
        )));
    }
    checkout_path(repository, label)
}

fn checkout_path(repository: &RepositoryDefinition, label: &str) -> Result<PathBuf, AppError> {
    let lexical = repository.root().join(repository.display_path());
    resolve_entry_path(&lexical, repository.root()).map_err(|error| match error {
        ResolutionError::OutsideRoot => AppError::config_error(format!(
            "{label}: repository '{}' path leaves the grove root",
            repository.name()
        )),
        ResolutionError::Io(err) => err.into(),
    })
}

/// Converging a leaving declaration moves or trashes its whole directory, so
/// none may hold, or sit inside, a path the configuration that follows
/// declares, nor hold a file that configuration is read from, whether as
/// written or where it leads.
pub(super) fn validate_containment<'d, 'a: 'd>(
    leaving: &RemovedRepository,
    declared: impl IntoIterator<Item = &'d Declaration<'a>>,
    sources: &[PathBuf],
    change: Change,
    label: &str,
) -> Result<(), AppError> {
    let path = on_disk(leaving.path())?;
    let overlapping = declared
        .into_iter()
        .flat_map(|kept| [&kept.route, &kept.location])
        .find(|kept| kept.starts_with(&path) || path.starts_with(kept));
    if let Some(kept) = overlapping {
        return Err(AppError::config_error(format!(
            "{label}: {} '{}' would affect '{}', which remains configured",
            change.acting(),
            path.display(),
            kept.display()
        )));
    }
    if let Some(source) = sources.iter().find(|source| source.starts_with(&path)) {
        return Err(AppError::config_error(format!(
            "{label}: {} '{}' would remove '{}', which the remaining configuration is read from",
            change.acting(),
            path.display(),
            source.display()
        )));
    }
    Ok(())
}

/// The path with its leading part that holds no symbolic link spelled as the
/// filesystem spells it, and the rest as written. Comparisons then agree with
/// canonical paths on case-insensitive volumes, while a path through a link
/// keeps naming the link, as assessment treats it.
pub(super) fn on_disk(path: &Path) -> Result<PathBuf, AppError> {
    let mut resolved = PathBuf::new();
    let mut components = path.components();
    for component in components.by_ref() {
        let candidate = resolved.join(component);
        match candidate.symlink_metadata() {
            Ok(metadata) if !metadata.file_type().is_symlink() => {
                resolved = candidate.canonicalize()?;
            }
            Ok(_) => {
                resolved = candidate;
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                resolved = candidate;
                break;
            }
            Err(err) => return Err(err.into()),
        }
    }
    resolved.extend(components);
    Ok(resolved)
}

/// A path both configurations declare, the next one for a live repository
/// under a URL the current one does not declare there.
#[derive(Debug, Clone)]
pub(crate) struct Repoint {
    repository: RepositoryDefinition,
    path: PathBuf,
    previous: RemoteUrl,
}

impl Repoint {
    /// The repository as the next configuration declares it.
    pub(crate) fn repository(&self) -> &RepositoryDefinition {
        &self.repository
    }

    /// The checkout's path, resolved as a removed declaration's path is.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The URL the current configuration declares at the path.
    pub(crate) fn previous(&self) -> &RemoteUrl {
        &self.previous
    }
}

/// What replacing the current configuration with the next one asks of the
/// disk, path by path. URLs agree only when equal exactly, as removal requires
/// before it moves or trashes anything. A live repository declared where the
/// current configuration declares nothing is left for `gv sync` to clone, and
/// a removed declaration both configurations hold unchanged is left for it to
/// converge.
pub(super) struct Transition {
    /// Declarations at paths the next configuration no longer declares,
    /// converged as removed declarations.
    pub(super) leaving: Vec<RemovedRepository>,
    /// Removed declarations of the next configuration the current one does not
    /// hold as they are.
    pub(super) tombstones: Vec<RemovedRepository>,
    pub(super) repoints: Vec<Repoint>,
}

impl Transition {
    pub(super) fn replacing(
        current: &ResolvedConfig,
        next: &ResolvedConfig,
        root: &Path,
        sources: &[PathBuf],
        label: &str,
    ) -> Result<Self, AppError> {
        let previous = declarations(current, root)?;
        let declared = declarations(next, root)?;

        let mut entering = Vec::new();
        let mut tombstones = Vec::new();
        let mut repoints = Vec::new();
        for declaration in &declared {
            let before = declared_at(&previous, &declaration.location);
            match declaration.declared {
                Declared::Live(repository) => match before {
                    Some(before) if before.url().matches(repository.url()) => {}
                    Some(before) => repoints.push(Repoint {
                        repository: repository.clone(),
                        path: checkout_path(repository, label)?,
                        previous: before.url().clone(),
                    }),
                    None => entering.push(repository),
                },
                Declared::Removed(removed) => match before {
                    Some(Declared::Removed(before)) if before.url().matches(removed.url()) => {}
                    _ => tombstones.push(removed.clone()),
                },
            }
        }

        let departing = previous
            .iter()
            .filter(|declaration| declared_at(&declared, &declaration.location).is_none())
            .map(|declaration| declaration.declared)
            .collect::<Vec<_>>();
        let mut leaving = Vec::new();
        for departed in &departing {
            leaving.push(match *departed {
                Declared::Live(repository) => RemovedRepository::new(
                    repository.name().clone(),
                    entry_path(repository, Change::Replacement, label)?,
                    repository.display_path().to_string(),
                    repository.url().clone(),
                    inferred_destination(repository, &entering, &departing, next),
                ),
                Declared::Removed(removed) => RemovedRepository::new(
                    removed.name().clone(),
                    removed.path().to_path_buf(),
                    removed.display_path().to_string(),
                    removed.url().clone(),
                    removed.moved_to().filter(|target| is_live(next, target)).cloned(),
                ),
            });
        }

        // An absent path is neither moved nor trashed, so it constrains no path
        // declared beneath it; a repository becoming a group of the same name
        // could otherwise never be replaced, even once its directory is gone.
        for removed in &leaving {
            if exists(removed.path())? {
                validate_containment(removed, &declared, sources, Change::Replacement, label)?;
            }
        }
        for removed in &tombstones {
            if exists(removed.path())? {
                let own = on_disk(removed.path())?;
                let others = declared.iter().filter(|kept| kept.location != own);
                validate_containment(removed, others, sources, Change::Replacement, label)?;
            }
        }

        Ok(Self { leaving, tombstones, repoints })
    }
}

fn declared_at<'a>(declarations: &[Declaration<'a>], location: &Path) -> Option<Declared<'a>> {
    declarations
        .iter()
        .find(|declaration| declaration.location == location)
        .map(|declaration| declaration.declared)
}

fn is_live(config: &ResolvedConfig, name: &RepositoryName) -> bool {
    config.repositories().iter().any(|repository| repository.name() == name)
}

fn exists(path: &Path) -> Result<bool, AppError> {
    match path.symlink_metadata() {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

/// A leaving repository moves into the entering one declaring its URL only
/// while that pairing is unambiguous: one entering repository declares the
/// URL, no other leaving one does, and no removed declaration already moves
/// there. A guess could hand one checkout's work to another's path, while a
/// leaving repository without a destination is trashed only when it holds no
/// local-only state.
fn inferred_destination(
    repository: &RepositoryDefinition,
    entering: &[&RepositoryDefinition],
    departing: &[Declared<'_>],
    next: &ResolvedConfig,
) -> Option<RepositoryName> {
    let mut candidates =
        entering.iter().filter(|entering| entering.url().matches(repository.url()));
    let candidate = candidates.next()?;
    let contested = candidates.next().is_some()
        || departing.iter().any(|other| match other {
            Declared::Live(other) => {
                other.name() != repository.name() && other.url().matches(repository.url())
            }
            Declared::Removed(removed) => removed.moved_to() == Some(candidate.name()),
        })
        || next.removed().iter().any(|removed| removed.moved_to() == Some(candidate.name()));
    (!contested).then(|| candidate.name().clone())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::{Transition, sources};
    use crate::AppError;
    use crate::config::{include, load, validation};

    const ALPHA: &str = "git@example.com:company/alpha.git";
    const BETA: &str = "git@example.com:company/beta.git";
    const OLD: &str = "git@example.com:company/old.git";

    /// A grove whose grove.toml is the current configuration.
    struct Grove {
        _root: TempDir,
        directory: PathBuf,
    }

    impl Grove {
        fn new(current: &str) -> Self {
            let root = TempDir::new().unwrap();
            let directory = root.path().canonicalize().unwrap();
            fs::write(directory.join("grove.toml"), current).unwrap();
            Self { _root: root, directory }
        }

        fn write(&self, relative: &str, contents: &str) {
            let path = self.directory.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }

        fn replacing(&self, candidate: &str) -> Result<Transition, AppError> {
            let root = self.directory.join("grove.toml");
            let current = load(Some(&root))?;
            let tree = include::load_tree_with_replacement(&root, &root, candidate)?;
            let sources = sources(&tree, None)?;
            let next = validation::resolve(tree)?;
            Transition::replacing(&current, &next, &self.directory, &sources, "candidate.toml")
        }
    }

    fn leaving(transition: &Transition) -> Vec<(&str, Option<&str>)> {
        transition
            .leaving
            .iter()
            .map(|removed| (removed.display_path(), removed.moved_to().map(|name| name.as_str())))
            .collect()
    }

    fn tombstones(transition: &Transition) -> Vec<&str> {
        transition.tombstones.iter().map(|removed| removed.display_path()).collect()
    }

    fn repoints(transition: &Transition) -> Vec<(&str, String, String)> {
        transition
            .repoints
            .iter()
            .map(|repoint| {
                (
                    repoint.repository().display_path(),
                    repoint.previous().as_process_argument().to_string(),
                    repoint.repository().url().as_process_argument().to_string(),
                )
            })
            .collect()
    }

    fn is_empty(transition: &Transition) -> bool {
        transition.leaving.is_empty()
            && transition.tombstones.is_empty()
            && transition.repoints.is_empty()
    }

    #[test]
    fn keeps_paths_declared_with_the_same_url() {
        let grove = Grove::new(&format!(
            "version = 2\n[repos]\nalpha = \"{ALPHA}\"\n[removed.repos]\nbeta = \"{BETA}\"\n"
        ));

        let transition = grove
            .replacing(&format!(
                "version = 2\n[repos.alpha]\nurl = \"{ALPHA}\"\ndefault_branch = \"develop\"\n\
                 [repos]\nbeta = \"{BETA}\"\n"
            ))
            .unwrap();

        assert!(is_empty(&transition));
    }

    #[test]
    fn repoints_a_path_declared_with_another_url() {
        let grove = Grove::new(&format!("version = 2\n[repos]\nalpha = \"{ALPHA}\"\n"));

        let transition =
            grove.replacing(&format!("version = 2\n[repos]\nalpha = \"{BETA}\"\n")).unwrap();

        assert_eq!(repoints(&transition), [("alpha", ALPHA.to_string(), BETA.to_string())]);
        assert_eq!(transition.repoints[0].path(), grove.directory.join("alpha"));
        assert!(transition.leaving.is_empty() && transition.tombstones.is_empty());
    }

    #[test]
    fn leaves_an_entering_repository_to_sync() {
        let grove = Grove::new("version = 2\n");

        let transition =
            grove.replacing(&format!("version = 2\n[repos]\nalpha = \"{ALPHA}\"\n")).unwrap();

        assert!(is_empty(&transition));
    }

    #[test]
    fn leaves_an_unchanged_tombstone_to_sync() {
        let grove = Grove::new(&format!("version = 2\n[removed.repos]\nold = \"{OLD}\"\n"));

        let transition = grove
            .replacing(&format!(
                "version = 2\n[repos]\nalpha = \"{ALPHA}\"\n[removed.repos]\nold = \"{OLD}\"\n"
            ))
            .unwrap();

        assert!(is_empty(&transition));
    }

    #[test]
    fn converges_every_other_tombstone_of_the_candidate() {
        let grove = Grove::new(&format!(
            "version = 2\n[repos]\nalpha = \"{ALPHA}\"\n[removed.repos]\nold = \"{OLD}\"\n"
        ));

        let transition = grove
            .replacing(&format!(
                "version = 2\n[removed.repos]\nalpha = \"{ALPHA}\"\nold = \"{BETA}\"\nnew = \"{OLD}\"\n"
            ))
            .unwrap();

        assert_eq!(tombstones(&transition), ["alpha", "old", "new"]);
        assert!(transition.leaving.is_empty() && transition.repoints.is_empty());
    }

    #[test]
    fn a_leaving_repository_moves_to_the_one_entering_with_its_url() {
        let grove = Grove::new(&format!("version = 2\n[repos]\nalpha = \"{ALPHA}\"\n"));

        let transition = grove
            .replacing(&format!("version = 2\n[groups.apps]\nrenamed = \"{ALPHA}\"\n"))
            .unwrap();

        assert_eq!(leaving(&transition), [("alpha", Some("renamed"))]);
        assert_eq!(transition.leaving[0].path(), grove.directory.join("alpha"));
    }

    #[test]
    fn an_ambiguous_url_infers_no_move() {
        let cases = [
            (
                format!("version = 2\n[repos]\nalpha = \"{ALPHA}\"\n"),
                format!("version = 2\n[groups.apps]\nfirst = \"{ALPHA}\"\nsecond = \"{ALPHA}\"\n"),
                vec![("alpha", None)],
            ),
            (
                format!("version = 2\n[repos]\nalpha = \"{ALPHA}\"\ncopy = \"{ALPHA}\"\n"),
                format!("version = 2\n[groups.apps]\nalpha = \"{ALPHA}\"\n"),
                vec![("alpha", None), ("copy", None)],
            ),
            (
                format!("version = 2\n[repos]\nalpha = \"{ALPHA}\"\n"),
                format!(
                    "version = 2\n[groups.apps]\nalpha = \"{ALPHA}\"\n\
                     [removed.repos]\nold = {{ url = \"{OLD}\", moved_to = \"alpha\" }}\n"
                ),
                vec![("alpha", None)],
            ),
            (
                format!(
                    "version = 2\n[repos]\nalpha = \"{ALPHA}\"\n\
                     [removed.repos]\nold = {{ url = \"{OLD}\", moved_to = \"alpha\" }}\n"
                ),
                format!("version = 2\n[groups.apps]\nalpha = \"{ALPHA}\"\n"),
                vec![("alpha", None), ("old", Some("alpha"))],
            ),
            (
                format!("version = 2\n[repos]\nalpha = \"{ALPHA}\"\n"),
                "version = 2\n[groups.apps]\nalpha = \"https://example.com/company/alpha.git\"\n"
                    .to_string(),
                vec![("alpha", None)],
            ),
        ];

        for (current, candidate, expected) in cases {
            let grove = Grove::new(&current);

            let transition = grove.replacing(&candidate).unwrap();

            assert_eq!(leaving(&transition), expected, "{candidate}");
        }
    }

    #[test]
    fn a_leaving_tombstone_keeps_only_a_destination_still_live() {
        let grove = Grove::new(&format!(
            "version = 2\n[repos]\nalpha = \"{ALPHA}\"\nbeta = \"{BETA}\"\n\
             [removed.repos]\nold = {{ url = \"{OLD}\", moved_to = \"alpha\" }}\n\
             older = {{ url = \"{OLD}\", moved_to = \"beta\" }}\n"
        ));

        let transition =
            grove.replacing(&format!("version = 2\n[repos]\nalpha = \"{ALPHA}\"\n")).unwrap();

        assert_eq!(leaving(&transition), [("beta", None), ("old", Some("alpha")), ("older", None)]);
    }

    #[test]
    fn fails_when_a_leaving_path_holds_a_path_the_candidate_declares() {
        let grove = Grove::new(&format!("version = 2\n[repos]\nWork = \"{ALPHA}\"\n"));
        fs::create_dir(grove.directory.join("Work")).unwrap();

        let error = grove
            .replacing(&format!(
                "version = 2\n[repos.inner]\nurl = \"{BETA}\"\npath = \"Work/inner\"\n"
            ))
            .err()
            .unwrap();

        assert!(error.to_string().contains("removing"), "{error}");
        assert!(error.to_string().contains("which remains configured"), "{error}");
    }

    #[test]
    fn an_absent_leaving_path_may_hold_a_path_the_candidate_declares() {
        let grove = Grove::new(&format!("version = 2\n[repos]\nfoo = \"{ALPHA}\"\n"));

        let transition =
            grove.replacing(&format!("version = 2\n[groups.foo]\na = \"{BETA}\"\n")).unwrap();

        assert_eq!(leaving(&transition), [("foo", None)]);
    }

    #[test]
    fn fails_when_a_leaving_or_new_removed_path_holds_a_file_the_candidate_reads() {
        for (current, candidate) in [
            (
                format!("version = 2\n[repos]\nfoo = \"{ALPHA}\"\n"),
                "version = 2\ninclude = [\"foo/repos.toml\"]\n".to_string(),
            ),
            (
                "version = 2\n".to_string(),
                format!(
                    "version = 2\ninclude = [\"foo/repos.toml\"]\n[removed.repos]\nfoo = \"{ALPHA}\"\n"
                ),
            ),
        ] {
            let grove = Grove::new(&current);
            grove.write("foo/repos.toml", "version = 2\n");

            let error = grove.replacing(&candidate).err().unwrap();

            assert!(
                error.to_string().contains("which the remaining configuration is read from"),
                "{error}"
            );
        }
    }

    #[test]
    fn refuses_to_remove_a_repository_at_the_grove_root() {
        let grove =
            Grove::new(&format!("version = 2\n[repos.root]\nurl = \"{ALPHA}\"\npath = \".\"\n"));

        let error = grove.replacing("version = 2\n").err().unwrap();

        assert!(error.to_string().contains("is the grove root, which cannot be removed"));
    }
}
