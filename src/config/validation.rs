use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use super::file::RawEntry;
use super::include::{LoadedConfigFile, LoadedConfigTree};
use super::resolved::{GroupDirectory, ResolvedConfig};
use crate::AppError;
use crate::repositories::{
    BranchName, RemoteUrl, RemovedRepository, RepositoryDefinition, RepositoryName,
    ResolutionError, normalize_lexically, resolve_entry_path, resolve_operational_path,
};

type PathResolution = fn(&Path, &Path) -> Result<PathBuf, ResolutionError>;

/// The fields live and removed entries share, validated alike.
struct ResolvedEntry {
    name: RepositoryName,
    path: PathBuf,
    display_path: String,
    url: RemoteUrl,
}

pub(super) fn resolve(tree: LoadedConfigTree) -> Result<ResolvedConfig, AppError> {
    let mut groups = Vec::new();
    let mut repositories = Vec::new();
    let mut names = HashMap::new();
    let mut paths = HashMap::new();

    for file in &tree.files {
        validate_version(file)?;
        groups.extend(resolve_group_directories(file, &tree.root_directory)?);

        for entry in &file.raw.repositories {
            let raw = &entry.repository;
            let resolved = resolve_entry(
                file,
                entry,
                raw.path.as_deref(),
                raw.url.as_deref(),
                &tree.root_directory,
                resolve_operational_path,
            )?;
            let default_branch =
                validate_default_branch(raw.default_branch.as_deref(), &file.path)?;
            claim_name(&mut names, &resolved.name, file, "repository")?;
            claim_path(&mut paths, &resolved, file)?;

            repositories.push(RepositoryDefinition::new(
                resolved.name,
                resolved.path,
                resolved.display_path,
                resolved.url,
                default_branch,
                file.path.clone(),
                tree.root_directory.clone(),
            ));
        }
    }

    let removed = resolve_removed(&tree, &names, &mut paths)?;

    let all_paths = repositories
        .iter()
        .map(RepositoryDefinition::path)
        .chain(removed.iter().map(RemovedRepository::path))
        .collect::<Vec<_>>();
    validate_no_nested_repository_paths(&all_paths)?;

    Ok(ResolvedConfig::new(tree.root_path, groups, repositories, removed))
}

/// Removed names share no namespace with live ones: moving a repository
/// between groups keeps its name, and the CLI never addresses a removed entry
/// by name. Their paths do share one, because sync acts on them. A destination
/// takes at most one removed entry, since only the first could move into it
/// and a dry run could not report what the others would then do.
fn resolve_removed(
    tree: &LoadedConfigTree,
    live_names: &HashMap<String, PathBuf>,
    paths: &mut HashMap<PathBuf, String>,
) -> Result<Vec<RemovedRepository>, AppError> {
    let mut removed = Vec::new();
    let mut names = HashMap::new();
    let mut destinations = HashMap::new();

    for file in &tree.files {
        for entry in &file.raw.removed {
            let raw = &entry.repository;
            let resolved = resolve_entry(
                file,
                entry,
                raw.path.as_deref(),
                raw.url.as_deref(),
                &tree.root_directory,
                resolve_entry_path,
            )?;
            let moved_to = raw
                .moved_to
                .as_deref()
                .map(|target| validate_moved_to(target, live_names, &file.path, &resolved.name))
                .transpose()?;
            claim_name(&mut names, &resolved.name, file, "removed repository")?;
            claim_path(paths, &resolved, file)?;
            if let Some(target) = &moved_to
                && let Some(existing) = destinations.insert(target.clone(), resolved.name.clone())
            {
                return Err(AppError::config_error(format!(
                    "{}: removed repositories '{existing}' and '{}' both move to '{target}'",
                    file.path.display(),
                    resolved.name
                )));
            }

            removed.push(RemovedRepository::new(
                resolved.name,
                resolved.path,
                resolved.display_path,
                resolved.url,
                moved_to,
            ));
        }
    }

    Ok(removed)
}

fn resolve_entry<T>(
    file: &LoadedConfigFile,
    entry: &RawEntry<T>,
    path: Option<&str>,
    url: Option<&str>,
    root: &Path,
    resolution: PathResolution,
) -> Result<ResolvedEntry, AppError> {
    let path = match path {
        Some(path) => required_field(Some(path), &file.path, &format!("{}.path", entry.field))?,
        None => entry.default_path.as_str(),
    };
    let url = required_field(url, &file.path, &format!("{}.url", entry.field))?;
    let name = RepositoryName::new(&entry.name)?;
    let (path, display_path) = resolve_repository_path(
        &file.directory,
        path,
        root,
        &file.path,
        name.as_str(),
        resolution,
    )?;
    let url = validate_url(url, &file.path, name.as_str())?;
    Ok(ResolvedEntry { name, path, display_path, url })
}

fn claim_name(
    names: &mut HashMap<String, PathBuf>,
    name: &RepositoryName,
    file: &LoadedConfigFile,
    kind: &str,
) -> Result<(), AppError> {
    match names.insert(name.as_str().to_string(), file.path.clone()) {
        Some(existing) => Err(AppError::config_error(format!(
            "{}: duplicate {kind} name '{name}' already defined in {}",
            file.path.display(),
            existing.display()
        ))),
        None => Ok(()),
    }
}

fn claim_path(
    paths: &mut HashMap<PathBuf, String>,
    entry: &ResolvedEntry,
    file: &LoadedConfigFile,
) -> Result<(), AppError> {
    match paths.insert(entry.path.clone(), entry.name.as_str().to_string()) {
        Some(existing_name) => Err(AppError::config_error(format!(
            "{}: duplicate repository path '{}' also used by '{existing_name}'",
            file.path.display(),
            entry.path.display()
        ))),
        None => Ok(()),
    }
}

fn validate_moved_to(
    target: &str,
    live_names: &HashMap<String, PathBuf>,
    source: &Path,
    name: &RepositoryName,
) -> Result<RepositoryName, AppError> {
    match RepositoryName::new(target) {
        Ok(target) if live_names.contains_key(target.as_str()) => Ok(target),
        _ => Err(AppError::config_error(format!(
            "{}: removed repository '{name}' moves to '{target}', which is not a configured repository",
            source.display()
        ))),
    }
}

fn resolve_group_directories(
    file: &LoadedConfigFile,
    root: &Path,
) -> Result<Vec<GroupDirectory>, AppError> {
    let mut directories = HashMap::<PathBuf, String>::new();
    let mut groups = Vec::new();

    for group in &file.raw.groups {
        let lexical = normalize_lexically(&file.directory.join(group));
        let resolved = match resolve_operational_path(&lexical, root) {
            Ok(path) => path,
            Err(ResolutionError::OutsideRoot) => {
                return Err(AppError::config_error(format!(
                    "{}: group directory '{group}' leaves the grove root",
                    file.path.display()
                )));
            }
            Err(ResolutionError::Io(err)) => return Err(err.into()),
        };

        if let Some(existing) = directories.get(&resolved) {
            if existing != group {
                return Err(AppError::config_error(format!(
                    "{}: group directories '{existing}' and '{group}' resolve to the same location",
                    file.path.display()
                )));
            }
        } else {
            directories.insert(resolved.clone(), group.to_string());
        }

        groups.push(GroupDirectory::new(resolved, relative_display(root, &lexical)));
    }

    Ok(groups)
}

fn validate_version(file: &LoadedConfigFile) -> Result<(), AppError> {
    match file.raw.version {
        Some(2) => Ok(()),
        Some(version) => Err(AppError::config_error(format!(
            "{}: unsupported config version {version}",
            file.path.display()
        ))),
        None => Err(AppError::config_error(format!(
            "{}: missing required field 'version'",
            file.path.display()
        ))),
    }
}

fn required_field<'a>(
    value: Option<&'a str>,
    source: &Path,
    field: &str,
) -> Result<&'a str, AppError> {
    let Some(value) = value else {
        return Err(AppError::config_error(format!(
            "{}: missing required field '{field}'",
            source.display()
        )));
    };
    if value.trim().is_empty() {
        return Err(AppError::config_error(format!(
            "{}: field '{field}' must not be empty",
            source.display()
        )));
    }
    Ok(value)
}

fn validate_default_branch(
    value: Option<&str>,
    source: &Path,
) -> Result<Option<BranchName>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    BranchName::new(value).map(Some).map_err(|err| {
        AppError::config_error(format!("{}: default_branch: {err}", source.display()))
    })
}

fn validate_url(url: &str, source: &Path, name: &str) -> Result<RemoteUrl, AppError> {
    RemoteUrl::new(url).map_err(|_| {
        AppError::config_error(format!(
            "{}: repository '{name}' has an invalid URL",
            source.display()
        ))
    })
}

fn resolve_repository_path(
    base: &Path,
    path: &str,
    root: &Path,
    source: &Path,
    name: &str,
    resolution: PathResolution,
) -> Result<(PathBuf, String), AppError> {
    let path = Path::new(path);
    if path.is_absolute() {
        return Err(AppError::config_error(format!(
            "{}: repository '{name}' path must be relative",
            source.display()
        )));
    }

    let lexical = normalize_lexically(&base.join(path));
    if !lexical.starts_with(root) {
        return Err(AppError::config_error(format!(
            "{}: repository '{name}' path leaves the grove root",
            source.display()
        )));
    }

    let resolved = match resolution(&lexical, root) {
        Ok(path) => path,
        Err(ResolutionError::OutsideRoot) => {
            return Err(AppError::config_error(format!(
                "{}: repository '{name}' path leaves the grove root",
                source.display()
            )));
        }
        Err(ResolutionError::Io(err)) => return Err(err.into()),
    };

    Ok((resolved, relative_display(root, &lexical)))
}

fn validate_no_nested_repository_paths(paths: &[&Path]) -> Result<(), AppError> {
    for (index, left) in paths.iter().enumerate() {
        for right in paths.iter().skip(index + 1) {
            if left.starts_with(right) || right.starts_with(left) {
                return Err(AppError::config_error(format!(
                    "repository paths must not be nested: '{}' and '{}'",
                    left.display(),
                    right.display()
                )));
            }
        }
    }
    Ok(())
}

fn relative_display(root: &Path, path: &Path) -> String {
    let display = path
        .strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if display.is_empty() { ".".to_string() } else { display }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_parent_components_inside_root() {
        let root = Path::new("/workspace");
        let base = Path::new("/workspace/work");

        let resolved = resolve_repository_path(
            base,
            "../shared/repo",
            root,
            Path::new("grove.toml"),
            "repo",
            resolve_operational_path,
        )
        .unwrap();

        assert_eq!(resolved.0, PathBuf::from("/workspace/shared/repo"));
    }

    #[test]
    fn rejects_paths_outside_root() {
        let root = Path::new("/workspace");
        let base = Path::new("/workspace/work");

        let result = resolve_repository_path(
            base,
            "../../outside",
            root,
            Path::new("grove.toml"),
            "repo",
            resolve_operational_path,
        );

        let error = result.expect_err("path outside the root should fail");
        assert_eq!(error.kind(), crate::AppErrorKind::Configuration);
    }

    #[test]
    fn displays_the_grove_root_as_dot() {
        assert_eq!(relative_display(Path::new("/workspace"), Path::new("/workspace")), ".");
    }
}
