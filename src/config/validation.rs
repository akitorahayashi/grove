use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use super::include::{LoadedConfigFile, LoadedConfigTree};
use super::resolved::ResolvedConfig;
use crate::AppError;
use crate::repositories::{RepositoryDefinition, RepositoryName};

pub(super) fn resolve(tree: LoadedConfigTree) -> Result<ResolvedConfig, AppError> {
    let mut repositories = Vec::new();
    let mut names = HashMap::new();
    let mut paths = HashMap::new();

    for file in &tree.files {
        validate_version(file)?;

        for raw in &file.raw.repositories {
            let name = required_field(raw.name.as_deref(), &file.path, "repo.name")?;
            let path = required_field(raw.path.as_deref(), &file.path, "repo.path")?;
            let url = required_field(raw.url.as_deref(), &file.path, "repo.url")?;
            let repository_name = RepositoryName::new(name)?;
            let resolved_path = resolve_repository_path(
                &file.directory,
                path,
                &tree.root_directory,
                &file.path,
                repository_name.as_str(),
            )?;
            let display_path = relative_display(&tree.root_directory, &resolved_path);
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
            ));
        }
    }

    validate_no_nested_repository_paths(&repositories)?;

    Ok(ResolvedConfig::new(tree.root_path, tree.root_directory, repositories))
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

fn validate_default_branch(value: Option<&str>, source: &Path) -> Result<Option<String>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.trim().is_empty() {
        return Err(AppError::config_error(format!(
            "{}: default_branch must not be empty",
            source.display()
        )));
    }
    Ok(Some(value.to_string()))
}

fn validate_url(url: &str, source: &Path, name: &str) -> Result<String, AppError> {
    if url.chars().any(char::is_control) {
        return Err(AppError::config_error(format!(
            "{}: repository '{name}' has an invalid URL",
            source.display()
        )));
    }
    Ok(url.to_string())
}

fn resolve_repository_path(
    base: &Path,
    path: &str,
    root: &Path,
    source: &Path,
    name: &str,
) -> Result<PathBuf, AppError> {
    let path = Path::new(path);
    if path.is_absolute() {
        return Err(AppError::config_error(format!(
            "{}: repository '{name}' path must be relative",
            source.display()
        )));
    }

    let resolved = normalize_path(&base.join(path));
    if !resolved.starts_with(root) {
        return Err(AppError::config_error(format!(
            "{}: repository '{name}' path leaves the grove root",
            source.display()
        )));
    }

    Ok(resolved)
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

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(std::path::MAIN_SEPARATOR.to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
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

        assert_eq!(resolved, PathBuf::from("/workspace/shared/repo"));
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
