use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use super::include::{LoadedConfigFile, LoadedConfigTree};
use super::resolved::ResolvedConfig;
use crate::AppError;
use crate::repositories::{
    BranchName, RemoteUrl, RepositoryDefinition, RepositoryName, ResolutionError,
    normalize_lexically, resolve_operational_path,
};

pub(super) fn resolve(tree: LoadedConfigTree) -> Result<ResolvedConfig, AppError> {
    let mut repositories = Vec::new();
    let mut names = HashMap::new();
    let mut paths = HashMap::new();

    for file in &tree.files {
        validate_version(file)?;

        for entry in &file.raw.repositories {
            let name = entry.name.as_str();
            let raw = &entry.repository;
            let path = match raw.path.as_deref() {
                Some(path) => {
                    required_field(Some(path), &file.path, &format!("repos.{name}.path"))?
                }
                None => name,
            };
            let url = required_field(raw.url.as_deref(), &file.path, &format!("repos.{name}.url"))?;
            let repository_name = RepositoryName::new(name)?;
            let (resolved_path, display_path) = resolve_repository_path(
                &file.directory,
                path,
                &tree.root_directory,
                &file.path,
                repository_name.as_str(),
            )?;
            let source_config = file.path.clone();
            let default_branch =
                validate_default_branch(raw.default_branch.as_deref(), &file.path)?;
            let url = validate_url(url, &file.path, repository_name.as_str())?;

            if let Some(existing) =
                names.insert(repository_name.as_str().to_string(), file.path.clone())
            {
                return Err(AppError::config_error(format!(
                    "{}: duplicate repository name '{}' already defined in {}",
                    file.path.display(),
                    repository_name,
                    existing.display()
                )));
            }
            if let Some(existing_name) =
                paths.insert(resolved_path.clone(), repository_name.as_str().to_string())
            {
                return Err(AppError::config_error(format!(
                    "{}: duplicate repository path '{}' also used by '{}'",
                    file.path.display(),
                    resolved_path.display(),
                    existing_name
                )));
            }

            repositories.push(RepositoryDefinition::new(
                repository_name,
                resolved_path,
                display_path,
                url,
                default_branch,
                source_config,
                tree.root_directory.clone(),
            ));
        }
    }

    validate_no_nested_repository_paths(&repositories)?;

    Ok(ResolvedConfig::new(tree.root_path, repositories))
}

fn validate_version(file: &LoadedConfigFile) -> Result<(), AppError> {
    match file.raw.version {
        Some(1) => Ok(()),
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

    let resolved = match resolve_operational_path(&lexical, root) {
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

fn validate_no_nested_repository_paths(
    repositories: &[RepositoryDefinition],
) -> Result<(), AppError> {
    for (index, left) in repositories.iter().enumerate() {
        for right in repositories.iter().skip(index + 1) {
            if left.path().starts_with(right.path()) || right.path().starts_with(left.path()) {
                return Err(AppError::config_error(format!(
                    "repository paths must not be nested: '{}' and '{}'",
                    left.path().display(),
                    right.path().display()
                )));
            }
        }
    }
    Ok(())
}

fn relative_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_parent_components_inside_root() {
        let root = Path::new("/workspace");
        let base = Path::new("/workspace/work");

        let resolved =
            resolve_repository_path(base, "../shared/repo", root, Path::new("grove.toml"), "repo")
                .unwrap();

        assert_eq!(resolved.0, PathBuf::from("/workspace/shared/repo"));
    }

    #[test]
    fn rejects_paths_outside_root() {
        let root = Path::new("/workspace");
        let base = Path::new("/workspace/work");

        let result =
            resolve_repository_path(base, "../../outside", root, Path::new("grove.toml"), "repo");

        assert!(matches!(result, Err(AppError::ConfigError(_))));
    }
}
