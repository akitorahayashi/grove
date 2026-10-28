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
    RemovedRepository, RepositoryDefinition, RepositoryName, ResolutionError, resolve_entry_path,
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

        let remaining = include::load_tree_without_override_of(&root_path, &base)
            .and_then(validation::resolve)
            .map_err(|error| {
                AppError::config_source(
                    format!(
                        "{display_path}: retiring it would leave an invalid configuration: {error}"
                    ),
                    error,
                )
            })?;
        let retiring = retiring(&current, &remaining, &display_path)?;
        validate_no_overlap(&retiring, &remaining, &display_path)?;

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
        .map_err(|err| AppError::config_source(format!("{}: {err}", target.display()), err))?;
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

/// Every declaration whose path the remaining configuration no longer
/// declares. A live repository the remaining configuration keeps under another
/// path moves back there, and a removed declaration keeps its destination only
/// while that destination remains.
fn retiring(
    current: &ResolvedConfig,
    remaining: &ResolvedConfig,
    label: &str,
) -> Result<Vec<RemovedRepository>, AppError> {
    let kept = declared_paths(remaining);
    let remains = |name: &RepositoryName| {
        remaining.repositories().iter().any(|repository| repository.name() == name)
    };

    let mut retiring = Vec::new();
    for repository in current.repositories() {
        if kept.contains(&repository.path()) {
            continue;
        }
        retiring.push(RemovedRepository::new(
            repository.name().clone(),
            entry_path(repository, label)?,
            repository.display_path().to_string(),
            repository.url().clone(),
            remains(repository.name()).then(|| repository.name().clone()),
        ));
    }
    for removed in current.removed() {
        if kept.contains(&removed.path()) {
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
/// inside, a path the remaining configuration still declares.
fn validate_no_overlap(
    retiring: &[RemovedRepository],
    remaining: &ResolvedConfig,
    label: &str,
) -> Result<(), AppError> {
    for kept in declared_paths(remaining) {
        let overlapping = retiring
            .iter()
            .find(|retired| retired.path().starts_with(kept) || kept.starts_with(retired.path()));
        if let Some(retired) = overlapping {
            return Err(AppError::config_error(format!(
                "{label}: retiring '{}' would affect '{}', which remains configured",
                retired.path().display(),
                kept.display()
            )));
        }
    }
    Ok(())
}

fn declared_paths(config: &ResolvedConfig) -> Vec<&Path> {
    config
        .repositories()
        .iter()
        .map(RepositoryDefinition::path)
        .chain(config.removed().iter().map(RemovedRepository::path))
        .collect()
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
