use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::path::{Component, Path};
use toml::{Table, Value};

use crate::AppError;

#[derive(Debug)]
pub(super) struct RawConfigFile {
    pub version: Option<u32>,
    pub include: Vec<String>,
    pub groups: Vec<String>,
    pub repositories: Vec<RawEntry<RawRepository>>,
    pub removed: Vec<RawEntry<RawRemovedRepository>>,
}

#[derive(Debug)]
pub(super) struct RawEntry<T> {
    pub name: String,
    pub default_path: String,
    pub field: String,
    pub repository: T,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawRepository {
    pub path: Option<String>,
    pub url: Option<String>,
    pub default_branch: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawRemovedRepository {
    pub path: Option<String>,
    pub url: Option<String>,
    pub moved_to: Option<String>,
}

/// A URL string is shorthand for a table holding only `url`, for live and
/// removed entries alike.
trait FromUrl {
    fn from_url(url: String) -> Self;
}

impl FromUrl for RawRepository {
    fn from_url(url: String) -> Self {
        Self { path: None, url: Some(url), default_branch: None }
    }
}

impl FromUrl for RawRemovedRepository {
    fn from_url(url: String) -> Self {
        Self { path: None, url: Some(url), moved_to: None }
    }
}

/// Parses TOML text into a table without decoding it, so a caller can deep
/// merge a sibling override in before the schema (and its `deny_unknown_fields`
/// checks) ever sees the combined document.
pub(super) fn parse_table(contents: &str, label: &str) -> Result<Table, AppError> {
    contents
        .parse::<Table>()
        .map_err(|err| super::source_error(format!("{label}: invalid TOML: {err}"), err))
}

/// Merges `overlay` into `base` in place: nested tables merge recursively so a
/// deeply nested key can be overridden without restating its siblings, while
/// scalars and arrays are replaced wholesale. Replacing arrays outright (rather
/// than concatenating) keeps `include` overridable to a shorter list instead of
/// only ever growing.
pub(super) fn merge_tables(base: &mut Table, overlay: Table) {
    for (key, overlay_value) in overlay {
        match base.get_mut(&key) {
            Some(base_value) => merge_value(base_value, overlay_value),
            None => {
                base.insert(key, overlay_value);
            }
        }
    }
}

pub(super) fn expand_repository_strings(root: &mut Table) {
    expand_section(root);
    if let Some(Value::Table(removed)) = root.get_mut("removed") {
        expand_section(removed);
    }
}

fn expand_section(section: &mut Table) {
    if let Some(Value::Table(repositories)) = section.get_mut("repos") {
        expand_strings(repositories);
    }
    if let Some(Value::Table(groups)) = section.get_mut("groups") {
        for (_, group) in groups.iter_mut() {
            if let Value::Table(repositories) = group {
                expand_strings(repositories);
            }
        }
    }
}

fn expand_strings(repositories: &mut Table) {
    for (_, repository) in repositories.iter_mut() {
        let Some(url) = repository.as_str().map(str::to_string) else {
            continue;
        };
        let mut fields = Table::new();
        fields.insert("url".to_string(), Value::from(url));
        *repository = Value::Table(fields);
    }
}

fn merge_value(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Table(base), Value::Table(overlay)) => merge_tables(base, overlay),
        (base, overlay) => *base = overlay,
    }
}

pub(super) fn decode(mut root: Table, label: &str) -> Result<RawConfigFile, AppError> {
    reject_unknown_root_fields(&root, label)?;

    let version = parse_version(root.remove("version"), label)?;
    let include = parse_include(root.remove("include"), label)?;
    let mut repositories = parse_repositories(root.remove("repos"), "repos", label)?;
    let (groups, grouped_repositories) = parse_groups(root.remove("groups"), "groups", label)?;
    repositories.extend(grouped_repositories);
    let removed = parse_removed(root.remove("removed"), label)?;

    Ok(RawConfigFile { version, include, groups, repositories, removed })
}

/// Removed groups are not recorded as group directories: a group that only
/// holds removed entries is on its way out and must not steer `gv add`.
fn parse_removed(
    value: Option<Value>,
    label: &str,
) -> Result<Vec<RawEntry<RawRemovedRepository>>, AppError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Value::Table(mut section) = value else {
        return Err(AppError::config_error(format!("{label}: field 'removed' must be a table")));
    };
    for key in section.keys() {
        if !matches!(key.as_str(), "repos" | "groups") {
            return Err(AppError::config_error(format!("{label}: unknown field `removed.{key}`")));
        }
    }
    let mut removed = parse_repositories(section.remove("repos"), "removed.repos", label)?;
    let (_, grouped) = parse_groups(section.remove("groups"), "removed.groups", label)?;
    removed.extend(grouped);
    Ok(removed)
}

fn reject_unknown_root_fields(root: &Table, label: &str) -> Result<(), AppError> {
    for key in root.keys() {
        if !matches!(key.as_str(), "version" | "include" | "repos" | "groups" | "removed") {
            return Err(AppError::config_error(format!("{label}: unknown field `{key}`")));
        }
    }
    Ok(())
}

fn parse_version(value: Option<Value>, label: &str) -> Result<Option<u32>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(version) = value.as_integer() else {
        return Err(AppError::config_error(format!("{label}: field 'version' must be an integer")));
    };
    let version = u32::try_from(version).map_err(|_| {
        AppError::config_error(format!("{label}: field 'version' must be a supported integer"))
    })?;
    Ok(Some(version))
}

fn parse_include(value: Option<Value>, label: &str) -> Result<Vec<String>, AppError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value.try_into().map_err(|err| super::source_error(format!("{label}: include: {err}"), err))
}

fn parse_repositories<T: DeserializeOwned + FromUrl>(
    value: Option<Value>,
    field: &str,
    label: &str,
) -> Result<Vec<RawEntry<T>>, AppError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Value::Table(table) = value else {
        return Err(AppError::config_error(format!("{label}: field '{field}' must be a table")));
    };

    parse_repository_table(table, field, None, label)
}

fn parse_groups<T: DeserializeOwned + FromUrl>(
    value: Option<Value>,
    field: &str,
    label: &str,
) -> Result<(Vec<String>, Vec<RawEntry<T>>), AppError> {
    let Some(value) = value else {
        return Ok((Vec::new(), Vec::new()));
    };
    let Value::Table(groups) = value else {
        return Err(AppError::config_error(format!("{label}: field '{field}' must be a table")));
    };

    let mut directories = Vec::new();
    let mut entries = Vec::new();
    for (directory, value) in groups {
        validate_group_directory(&directory, label)?;
        let Value::Table(repositories) = value else {
            return Err(AppError::config_error(format!(
                "{label}: group '{directory}' must be a table"
            )));
        };
        directories.push(directory.clone());
        entries.extend(parse_repository_table(repositories, field, Some(&directory), label)?);
    }
    Ok((directories, entries))
}

fn validate_group_directory(directory: &str, label: &str) -> Result<(), AppError> {
    let path = Path::new(directory);
    let normalized = path
        .components()
        .map(|component| match component {
            Component::Normal(part) => part.to_str().map(str::to_string),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.join("/"));
    if path.is_absolute() || normalized.as_deref() != Some(directory) || directory.trim().is_empty()
    {
        return Err(AppError::config_error(format!(
            "{label}: group directory '{directory}' must be a normalized relative path"
        )));
    }
    Ok(())
}

fn parse_repository_table<T: DeserializeOwned + FromUrl>(
    table: Table,
    section: &str,
    group: Option<&str>,
    label: &str,
) -> Result<Vec<RawEntry<T>>, AppError> {
    table
        .into_iter()
        .map(|(name, value)| {
            let field = match group {
                Some(group) => format!("{section}.{group}.{name}"),
                None => format!("{section}.{name}"),
            };
            let default_path = match group {
                Some(group) => format!("{group}/{name}"),
                None => name.clone(),
            };
            let repository = match value {
                Value::String(url) => T::from_url(url),
                Value::Table(fields) => fields.try_into().map_err(|err| {
                    super::source_error(format!("{label}: repository '{name}': {err}"), err)
                })?,
                _ => {
                    return Err(AppError::config_error(format!(
                        "{label}: repository '{name}' must be a URL string or table"
                    )));
                }
            };
            Ok(RawEntry { name, default_path, field, repository })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use toml::{Table, Value};

    use super::{decode, expand_repository_strings, merge_tables, parse_table};

    #[test]
    fn preserves_repository_declaration_order() {
        let table = parse_table(
            r#"
version = 2

[repos]
second = "git@example.com:second.git"

[groups.services]
first = "git@example.com:first.git"
"#,
            "grove.toml",
        )
        .unwrap();
        let config = decode(table, "grove.toml").unwrap();

        let names = config.repositories.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>();

        assert_eq!(names, ["second", "first"]);
        assert_eq!(config.repositories[0].default_path, "second");
        assert_eq!(config.repositories[1].default_path, "services/first");
    }

    #[test]
    fn merge_tables_recurses_into_nested_tables_and_replaces_scalars_and_arrays() {
        let mut base = parse_table(
            r#"
version = 2
include = ["work/grove.toml", "personal/grove.toml"]

[groups.apps]
frontend = "git@example.com:frontend.git"

[repos.backend]
url = "git@example.com:backend.git"
"#,
            "grove.toml",
        )
        .unwrap();
        let overlay = parse_table(
            r#"
include = ["work/grove.toml"]

[groups.apps.frontend]
default_branch = "develop"

[repos]
personal = "git@example.com:personal.git"
"#,
            "grove.override.toml",
        )
        .unwrap();

        expand_repository_strings(&mut base);
        merge_tables(&mut base, overlay);

        assert_eq!(base["version"].as_integer(), Some(2));
        assert_eq!(base["include"].as_array().unwrap(), &[Value::from("work/grove.toml")]);
        let frontend = base["groups"]["apps"]["frontend"].as_table().unwrap();
        assert_eq!(frontend["url"].as_str(), Some("git@example.com:frontend.git"));
        assert_eq!(frontend["default_branch"].as_str(), Some("develop"));
        assert_eq!(base["repos"]["backend"]["url"].as_str(), Some("git@example.com:backend.git"));
        assert_eq!(base["repos"]["personal"].as_str(), Some("git@example.com:personal.git"));
    }

    #[test]
    fn merge_tables_leaves_base_only_keys_untouched_by_an_empty_overlay() {
        let mut base = parse_table("version = 2\n", "grove.toml").unwrap();

        merge_tables(&mut base, Table::new());

        assert_eq!(base["version"].as_integer(), Some(2));
    }
}
