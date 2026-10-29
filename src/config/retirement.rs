use std::io;
use std::path::{Path, PathBuf};

use super::ResolvedConfig;
use super::discovery::locate;
use super::include::{
    self, LoadedConfigTree, resolve_sibling_override_path, sibling_override_path,
};
use super::lock::DirectoryLock;
use super::validation;
use crate::AppError;
use crate::repositories::{
    RemoteUrl, RemovedRepository, RepositoryDefinition, RepositoryName, ResolutionError,
    normalize_lexically, resolve_entry_path,
};

/// One override's retirement, settled under the root configuration's
/// directory lock: the declarations that leave with the override, expressed as
/// removed declarations, and the configuration that remains once it is gone.
pub(crate) struct Retirement {
    override_path: PathBuf,
    display_path: String,
    remaining: ResolvedConfig,
    retiring: Vec<RemovedRepository>,
    _lock: DirectoryLock,
}

impl Retirement {
    /// Without `target`, the root configuration's own override retires.
    pub(crate) fn open(
        explicit_config: Option<&Path>,
        target: Option<&Path>,
        dry_run: bool,
    ) -> Result<Self, AppError> {
        let root_path = locate(explicit_config)?;
        let root_directory = root_path
            .parent()
            .ok_or_else(|| {
                AppError::config_error(format!("{} has no parent", root_path.display()))
            })?
            .to_path_buf();
        let lock = DirectoryLock::acquire(&root_directory, dry_run)?;

        let tree = include::load_tree(&root_path)?;
        let base = match target {
            Some(target) => paired_base(&tree, target)?,
            None => tree.root_path.clone(),
        };
        let current = validation::resolve(tree)?;
        let override_path = sibling_override_path(&base);
        let display_path = override_path
            .strip_prefix(&root_directory)
            .unwrap_or(&override_path)
            .display()
            .to_string();
        if resolve_sibling_override_path(&base)?.is_none() {
            return Err(AppError::config_error(format!("{display_path}: no override to retire")));
        }

        let (remaining, sources) = include::load_tree_without_override_of(&root_path, &base)
            .and_then(|tree| {
                let sources = sources(&tree, &base)?;
                Ok((validation::resolve(tree)?, sources))
            })
            .map_err(|error| {
                super::source_error(
                    format!(
                        "{display_path}: retiring it would leave an invalid configuration: {error}"
                    ),
                    error,
                )
            })?;
        let declared = declarations(&remaining, &root_directory)?;
        let retiring = retiring(&current, &remaining, &declared, &display_path)?;
        validate_containment(&retiring, &declared, &sources, &display_path)?;

        Ok(Self { override_path, display_path, remaining, retiring, _lock: lock })
    }

    /// The override as it sits beside its base file; a symbolic link there is
    /// the link, so retiring it never touches the file it points to.
    pub(crate) fn override_path(&self) -> &Path {
        &self.override_path
    }

    pub(crate) fn display_path(&self) -> &str {
        &self.display_path
    }

    pub(crate) fn remaining(&self) -> &ResolvedConfig {
        &self.remaining
    }

    pub(crate) fn retiring(&self) -> &[RemovedRepository] {
        &self.retiring
    }
}

/// Symbolic links are resolved on both sides, so naming an override through a
/// link or through its target finds the same base file.
fn paired_base(tree: &LoadedConfigTree, target: &Path) -> Result<PathBuf, AppError> {
    let resolved = target
        .canonicalize()
        .map_err(|err| super::source_error(format!("{}: {err}", target.display()), err))?;
    tree.files
        .iter()
        .map(|file| &file.path)
        .find(|base| {
            sibling_override_path(base).canonicalize().is_ok_and(|candidate| candidate == resolved)
        })
        .cloned()
        .ok_or_else(|| {
            AppError::config_error(format!(
                "{}: not the override of {} or of a file it includes",
                target.display(),
                tree.root_path.display()
            ))
        })
}

/// The files the remaining configuration is read from, both through the paths
/// that name them and where those paths lead: every loaded base file and every
/// override still merged into one.
fn sources(tree: &LoadedConfigTree, retired_base: &Path) -> Result<Vec<PathBuf>, AppError> {
    let mut sources = Vec::new();
    for file in &tree.files {
        sources.push(file.path.clone());
        for include in &file.raw.include {
            sources.push(on_disk(&normalize_lexically(&file.directory.join(include)))?);
        }
        if file.path != retired_base
            && let Some(resolved) = resolve_sibling_override_path(&file.path)?
        {
            sources.push(on_disk(&sibling_override_path(&file.path))?);
            sources.push(resolved);
        }
    }
    Ok(sources)
}

/// A path the remaining configuration declares, with the repository declared
/// there: `route` is the path as written and `location` where it leads, both
/// spelled as `on_disk` spells them.
struct Declaration<'a> {
    route: PathBuf,
    location: PathBuf,
    url: &'a RemoteUrl,
}

fn declarations<'a>(
    config: &'a ResolvedConfig,
    root: &Path,
) -> Result<Vec<Declaration<'a>>, AppError> {
    config
        .repositories()
        .iter()
        .map(|repository| (repository.display_path(), repository.path(), repository.url()))
        .chain(
            config
                .removed()
                .iter()
                .map(|removed| (removed.display_path(), removed.path(), removed.url())),
        )
        .map(|(display_path, path, url)| {
            Ok(Declaration {
                route: on_disk(&root.join(display_path))?,
                location: on_disk(path)?,
                url,
            })
        })
        .collect()
}

/// Every declaration whose path the remaining configuration no longer
/// declares. A live repository the remaining configuration keeps under another
/// path moves back there only when both declare the same URL exactly: the
/// cache's `identity` would let another SSH account's repository be moved
/// and repointed. A removed declaration keeps its destination only while that
/// destination remains.
fn retiring(
    current: &ResolvedConfig,
    remaining: &ResolvedConfig,
    declared: &[Declaration<'_>],
    label: &str,
) -> Result<Vec<RemovedRepository>, AppError> {
    let remains = |name: &RepositoryName| {
        remaining.repositories().iter().any(|repository| repository.name() == name)
    };

    let mut retiring = Vec::new();
    for repository in current.repositories() {
        if is_kept(declared, repository.path(), repository.url(), repository.display_path(), label)?
        {
            continue;
        }
        let kept = remaining.repositories().iter().find(|kept| kept.name() == repository.name());
        let moved_to = match kept {
            Some(kept) if kept.url().matches(repository.url()) => Some(kept.name().clone()),
            Some(kept) => {
                let subject = format!("repository '{}'", repository.name());
                return Err(substitution(label, &subject, repository.url(), kept.url()));
            }
            None => None,
        };
        retiring.push(RemovedRepository::new(
            repository.name().clone(),
            entry_path(repository, label)?,
            repository.display_path().to_string(),
            repository.url().clone(),
            moved_to,
        ));
    }
    for removed in current.removed() {
        if is_kept(declared, removed.path(), removed.url(), removed.display_path(), label)? {
            continue;
        }
        retiring.push(RemovedRepository::new(
            removed.name().clone(),
            removed.path().to_path_buf(),
            removed.display_path().to_string(),
            removed.url().clone(),
            removed.moved_to().filter(|target| remains(target)).cloned(),
        ));
    }
    Ok(retiring)
}

/// Whether the remaining configuration declares `path` for the same
/// repository; declaring it for another one is a substitution.
fn is_kept(
    declared: &[Declaration<'_>],
    path: &Path,
    url: &RemoteUrl,
    display_path: &str,
    label: &str,
) -> Result<bool, AppError> {
    let path = on_disk(path)?;
    match declared.iter().find(|kept| kept.location == path) {
        None => Ok(false),
        Some(kept) if kept.url.same_identity(url) => Ok(true),
        Some(kept) => Err(substitution(label, &format!("'{display_path}'"), url, kept.url)),
    }
}

/// The override points `subject` at a different repository than the remaining
/// configuration does, so neither keeping nor moving its checkout would leave
/// the repository the remaining configuration expects.
fn substitution(label: &str, subject: &str, url: &RemoteUrl, kept: &RemoteUrl) -> AppError {
    AppError::config_error(format!(
        "{label}: {subject} points at {url} with the override but at {kept} without it; remove \
         that declaration from the override and settle the checkout by hand before retiring"
    ))
}

/// Resolves as a removed declaration's path does, so a symbolic link at the
/// repository's path is assessed as the link rather than as its target.
fn entry_path(repository: &RepositoryDefinition, label: &str) -> Result<PathBuf, AppError> {
    let name = repository.name();
    if repository.path() == repository.root() {
        return Err(AppError::config_error(format!(
            "{label}: repository '{name}' is the grove root, which cannot be retired"
        )));
    }
    let lexical = repository.root().join(repository.display_path());
    resolve_entry_path(&lexical, repository.root()).map_err(|error| match error {
        ResolutionError::OutsideRoot => AppError::config_error(format!(
            "{label}: repository '{name}' path leaves the grove root"
        )),
        ResolutionError::Io(err) => err.into(),
    })
}

/// Retiring moves or trashes whole directories, so none may hold, or sit
/// inside, a path the remaining configuration declares, nor hold a file that
/// configuration is read from, whether as written or where it leads.
fn validate_containment(
    retiring: &[RemovedRepository],
    declared: &[Declaration<'_>],
    sources: &[PathBuf],
    label: &str,
) -> Result<(), AppError> {
    for retired in retiring {
        let path = on_disk(retired.path())?;
        let overlapping = declared
            .iter()
            .flat_map(|kept| [&kept.route, &kept.location])
            .find(|kept| kept.starts_with(&path) || path.starts_with(kept));
        if let Some(kept) = overlapping {
            return Err(AppError::config_error(format!(
                "{label}: retiring '{}' would affect '{}', which remains configured",
                path.display(),
                kept.display()
            )));
        }
        if let Some(source) = sources.iter().find(|source| source.starts_with(&path)) {
            return Err(AppError::config_error(format!(
                "{label}: retiring '{}' would remove '{}', which the remaining configuration is \
                 read from",
                path.display(),
                source.display()
            )));
        }
    }
    Ok(())
}

/// The path with its leading part that holds no symbolic link spelled as the
/// filesystem spells it, and the rest as written. Comparisons then agree with
/// canonical paths on case-insensitive volumes, while a path through a link
/// keeps naming the link, as assessment treats it.
fn on_disk(path: &Path) -> Result<PathBuf, AppError> {
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::Retirement;
    use crate::AppError;

    const BASE: &str = "git@example.com:company/base.git";
    const WORK: &str = "git@example.com:company/work.git";
    const OLD: &str = "git@example.com:company/old.git";

    struct Grove {
        _root: TempDir,
        directory: PathBuf,
    }

    impl Grove {
        fn new(base: &str, overlay: Option<&str>) -> Self {
            let root = TempDir::new().unwrap();
            let directory = root.path().canonicalize().unwrap();
            fs::write(directory.join("grove.toml"), base).unwrap();
            if let Some(overlay) = overlay {
                fs::write(directory.join("grove.override.toml"), overlay).unwrap();
            }
            Self { _root: root, directory }
        }

        fn write(&self, relative: &str, contents: &str) {
            let path = self.directory.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }

        fn open(&self, target: Option<&Path>) -> Result<Retirement, AppError> {
            Retirement::open(Some(&self.directory.join("grove.toml")), target, false)
        }
    }

    fn base_with_one_repository() -> String {
        format!("version = 2\n[repos]\nbase = \"{BASE}\"\n")
    }

    fn retiring(retirement: &Retirement) -> Vec<(&str, Option<&str>)> {
        retirement
            .retiring()
            .iter()
            .map(|removed| (removed.display_path(), removed.moved_to().map(|name| name.as_str())))
            .collect()
    }

    fn remaining(retirement: &Retirement) -> Vec<&str> {
        retirement.remaining().repositories().iter().map(|entry| entry.name().as_str()).collect()
    }

    #[test]
    fn retires_the_repositories_only_the_override_declares() {
        let grove = Grove::new(
            &base_with_one_repository(),
            Some(&format!("[groups.Work]\nwork = \"{WORK}\"\n")),
        );

        let retirement = grove.open(None).unwrap();

        assert_eq!(retiring(&retirement), [("Work/work", None)]);
        assert_eq!(retirement.retiring()[0].path(), grove.directory.join("Work/work"));
        assert_eq!(remaining(&retirement), ["base"]);
        assert_eq!(retirement.override_path(), grove.directory.join("grove.override.toml"));
        assert_eq!(retirement.display_path(), "grove.override.toml");
    }

    #[test]
    fn a_repository_the_override_relocated_moves_back_to_its_base_path() {
        let grove =
            Grove::new(&base_with_one_repository(), Some("[repos.base]\npath = \"Work/base\"\n"));

        let retirement = grove.open(None).unwrap();

        assert_eq!(retiring(&retirement), [("Work/base", Some("base"))]);
    }

    #[test]
    fn an_override_changing_only_the_default_branch_retires_nothing() {
        let grove = Grove::new(
            &base_with_one_repository(),
            Some("[repos.base]\ndefault_branch = \"develop\"\n"),
        );

        let retirement = grove.open(None).unwrap();

        assert!(retirement.retiring().is_empty());
    }

    #[test]
    fn retires_removed_declarations_keeping_only_destinations_that_remain() {
        let grove = Grove::new(
            &base_with_one_repository(),
            Some(&format!(
                "[groups.Work]\nwork = \"{WORK}\"\n\n[removed.repos]\n\
                 old = {{ url = \"{OLD}\", moved_to = \"work\" }}\n\
                 older = {{ url = \"{OLD}\", moved_to = \"base\" }}\n"
            )),
        );

        let retirement = grove.open(None).unwrap();

        assert_eq!(
            retiring(&retirement),
            [("Work/work", None), ("old", None), ("older", Some("base"))]
        );
    }

    #[test]
    fn fails_when_retiring_would_leave_an_invalid_configuration() {
        let grove = Grove::new(&format!("[repos]\nbase = \"{BASE}\"\n"), Some("version = 2\n"));

        let error = grove.open(None).err().unwrap();

        assert!(error.to_string().contains("retiring it would leave an invalid configuration"));
        assert!(error.to_string().contains("missing required field 'version'"));
    }

    #[test]
    fn fails_when_a_retiring_path_overlaps_a_remaining_one() {
        let grove = Grove::new(
            &base_with_one_repository(),
            Some(&format!(
                "[repos.base]\npath = \"Work/base\"\n\n[repos.inner]\nurl = \"{WORK}\"\npath = \"base/inner\"\n"
            )),
        );

        let error = grove.open(None).err().unwrap();

        assert!(error.to_string().contains("which remains configured"));
    }

    #[test]
    fn fails_when_a_retiring_repository_holds_a_file_the_remaining_configuration_reads() {
        let grove = Grove::new(
            "version = 2\ninclude = [\"foo/repos.toml\"]\n",
            Some(&format!("[repos]\nfoo = \"{WORK}\"\n")),
        );
        grove.write("foo/repos.toml", "version = 2\n");

        let error = grove.open(None).err().unwrap();

        assert!(error.to_string().contains("which the remaining configuration is read from"));
    }

    #[test]
    fn fails_when_a_path_names_another_repository_without_the_override() {
        let grove = Grove::new(
            &format!("version = 2\n[repos.alpha]\nurl = \"{BASE}\"\npath = \"shared\"\n"),
            Some(&format!(
                "[repos.alpha]\npath = \"other\"\n\n[repos.beta]\nurl = \"{WORK}\"\npath = \"shared\"\n"
            )),
        );

        let error = grove.open(None).err().unwrap();

        assert!(error.to_string().contains("'shared' points at"));
    }

    #[test]
    fn fails_when_the_override_points_a_repository_at_another_url() {
        let grove = Grove::new(
            &base_with_one_repository(),
            Some(&format!("[repos.base]\nurl = \"{WORK}\"\n")),
        );

        let error = grove.open(None).err().unwrap();

        assert!(error.to_string().contains("'base' points at"));
    }

    #[test]
    fn fails_when_the_override_points_a_relocated_repository_at_another_url() {
        let grove = Grove::new(
            &base_with_one_repository(),
            Some(&format!("[repos.base]\nurl = \"{WORK}\"\npath = \"other\"\n")),
        );

        let error = grove.open(None).err().unwrap();

        assert!(error.to_string().contains("repository 'base' points at"));
    }

    /// The cache identity treats these as one repository, but moving the
    /// checkout back would repoint it at another account's repository.
    #[test]
    fn fails_when_the_override_relocates_a_repository_to_a_url_sharing_only_its_identity() {
        for url in ["bob@example.com:company/base.git", "https://example.com/company/base.git"] {
            let grove = Grove::new(
                &base_with_one_repository(),
                Some(&format!("[repos.base]\nurl = \"{url}\"\npath = \"other\"\n")),
            );

            let error = grove.open(None).err().unwrap();

            assert!(error.to_string().contains("repository 'base' points at"), "{url}: {error}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn fails_when_a_retiring_repository_holds_the_path_to_an_included_file() {
        let grove = Grove::new(
            "version = 2\ninclude = [\"Work/repos.toml\"]\n",
            Some(&format!("[repos]\nWork = \"{WORK}\"\n")),
        );
        grove.write("cfg/repos.toml", "version = 2\n");
        fs::create_dir(grove.directory.join("Work")).unwrap();
        std::os::unix::fs::symlink("../cfg/repos.toml", grove.directory.join("Work/repos.toml"))
            .unwrap();

        let error = grove.open(None).err().unwrap();

        assert!(error.to_string().contains("which the remaining configuration is read from"));
    }

    #[cfg(unix)]
    #[test]
    fn fails_when_a_retiring_repository_holds_the_path_to_a_remaining_repository() {
        let grove = Grove::new(
            &format!("version = 2\n[repos.kept]\nurl = \"{BASE}\"\npath = \"Work/link\"\n"),
            Some(&format!("[repos]\nWork = \"{WORK}\"\n")),
        );
        fs::create_dir(grove.directory.join("real")).unwrap();
        fs::create_dir(grove.directory.join("Work")).unwrap();
        std::os::unix::fs::symlink("../real", grove.directory.join("Work/link")).unwrap();

        let error = grove.open(None).err().unwrap();

        assert!(error.to_string().contains("which remains configured"));
    }

    #[test]
    fn a_path_declared_for_the_same_repository_without_the_override_stays() {
        let grove = Grove::new(
            &format!("version = 2\n[repos.alpha]\nurl = \"{BASE}\"\npath = \"shared\"\n"),
            Some(&format!(
                "[repos.alpha]\npath = \"other\"\n\n[repos.gamma]\nurl = \"{BASE}\"\npath = \"shared\"\n"
            )),
        );

        let retirement = grove.open(None).unwrap();

        assert_eq!(retiring(&retirement), [("other", Some("alpha"))]);
    }

    #[test]
    fn fails_for_a_file_that_is_not_an_override_of_the_grove() {
        let grove = Grove::new(&base_with_one_repository(), Some("version = 2\n"));

        let error = grove.open(Some(&grove.directory.join("grove.toml"))).err().unwrap();

        assert!(error.to_string().contains("not the override of"));
    }

    #[test]
    fn fails_without_an_override() {
        let grove = Grove::new(&base_with_one_repository(), None);

        let error = grove.open(None).err().unwrap();

        assert!(error.to_string().contains("grove.override.toml: no override to retire"));
    }

    #[test]
    fn retires_an_included_files_override_alone() {
        let grove = Grove::new(
            &format!(
                "version = 2\ninclude = [\"work/repositories.toml\"]\n[repos]\nbase = \"{BASE}\"\n"
            ),
            Some(&format!("[repos]\npersonal = \"{OLD}\"\n")),
        );
        grove.write("work/repositories.toml", "version = 2\n");
        grove.write("work/repositories.override.toml", &format!("[repos]\nwork = \"{WORK}\"\n"));

        let retirement =
            grove.open(Some(&grove.directory.join("work/repositories.override.toml"))).unwrap();

        assert_eq!(retiring(&retirement), [("work/work", None)]);
        assert_eq!(remaining(&retirement), ["base", "personal"]);
        assert_eq!(retirement.display_path(), "work/repositories.override.toml");
    }

    #[test]
    fn refuses_to_retire_a_repository_at_the_grove_root() {
        let grove = Grove::new(
            "version = 2\n",
            Some(&format!("[repos.root]\nurl = \"{WORK}\"\npath = \".\"\n")),
        );

        let error = grove.open(None).err().unwrap();

        assert!(error.to_string().contains("is the grove root"));
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_at_a_retiring_path_stays_the_link() {
        let grove = Grove::new("version = 2\n", Some(&format!("[repos]\nlink = \"{WORK}\"\n")));
        fs::create_dir(grove.directory.join("target")).unwrap();
        std::os::unix::fs::symlink(grove.directory.join("target"), grove.directory.join("link"))
            .unwrap();

        let retirement = grove.open(None).unwrap();

        assert_eq!(retirement.retiring()[0].path(), grove.directory.join("link"));
    }
}
