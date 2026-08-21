use serde::Deserialize;
use toml::{Table, Value};

use crate::AppError;

#[derive(Debug)]
pub(super) struct RawConfigFile {
    pub version: Option<u32>,
    pub include: Vec<String>,
    pub repositories: Vec<RawRepositoryEntry>,
}

#[derive(Debug)]
pub(super) struct RawRepositoryEntry {
    pub name: String,
    pub repository: RawRepository,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawRepository {
    pub path: Option<String>,
    pub url: Option<String>,
    pub default_branch: Option<String>,
}

/// Parses TOML text into a table without decoding it, so a caller can deep
/// merge a sibling override in before the schema (and its `deny_unknown_fields`
/// checks) ever sees the combined document.
pub(super) fn parse_table(contents: &str, label: &str) -> Result<Table, AppError> {
    contents
        .parse::<Table>()
        .map_err(|err| AppError::config_source(format!("{label}: invalid TOML: {err}"), err))
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
    let repositories = parse_repositories(root.remove("repos"), label)?;

    Ok(RawConfigFile { version, include, repositories })
}

fn reject_unknown_root_fields(root: &Table, label: &str) -> Result<(), AppError> {
    for key in root.keys() {
        if key == "repo" {
            return Err(AppError::config_error(format!(
                "{label}: unsupported field 'repo'; use [repos.<name>] tables"
            )));
        }
        if !matches!(key.as_str(), "version" | "include" | "repos") {
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
    value.try_into().map_err(|err| AppError::config_source(format!("{label}: include: {err}"), err))
}

fn parse_repositories(
    value: Option<Value>,
    label: &str,
) -> Result<Vec<RawRepositoryEntry>, AppError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Value::Table(table) = value else {
        return Err(AppError::config_error(format!("{label}: field 'repos' must be a table")));
    };

    table
        .into_iter()
        .map(|(name, value)| {
            let repository = value.try_into().map_err(|err| {
                AppError::config_source(format!("{label}: repository '{name}': {err}"), err)
            })?;
            Ok(RawRepositoryEntry { name, repository })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use toml::{Table, Value};

    use super::{decode, merge_tables, parse_table};

    #[test]
    fn preserves_repository_declaration_order() {
        let table = parse_table(
            r#"
version = 1

[repos.second]
url = "git@example.com:second.git"

[repos.first]
url = "git@example.com:first.git"
"#,
            "grove.toml",
        )
        .unwrap();
        let config = decode(table, "grove.toml").unwrap();

        let names = config.repositories.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>();

        assert_eq!(names, ["second", "first"]);
    }

    #[test]
    fn merge_tables_recurses_into_nested_tables_and_replaces_scalars_and_arrays() {
        let mut base = parse_table(
            r#"
version = 1
include = ["work/grove.toml", "personal/grove.toml"]

[repos.frontend]
path = "frontend"
url = "git@example.com:frontend.git"
default_branch = "main"

[repos.backend]
url = "git@example.com:backend.git"
"#,
            "grove.toml",
        )
        .unwrap();
        let overlay = parse_table(
            r#"
include = ["work/grove.toml"]

[repos.frontend]
path = "local/frontend"
default_branch = "develop"

[repos.personal]
url = "git@example.com:personal.git"
"#,
            "grove.override.toml",
        )
        .unwrap();

        merge_tables(&mut base, overlay);

        assert_eq!(base["version"].as_integer(), Some(1));
        assert_eq!(base["include"].as_array().unwrap(), &[Value::from("work/grove.toml")]);
        let frontend = base["repos"]["frontend"].as_table().unwrap();
        assert_eq!(frontend["path"].as_str(), Some("local/frontend"));
        assert_eq!(frontend["url"].as_str(), Some("git@example.com:frontend.git"));
        assert_eq!(frontend["default_branch"].as_str(), Some("develop"));
        assert_eq!(base["repos"]["backend"]["url"].as_str(), Some("git@example.com:backend.git"));
        assert_eq!(base["repos"]["personal"]["url"].as_str(), Some("git@example.com:personal.git"));
    }

    #[test]
    fn merge_tables_leaves_base_only_keys_untouched_by_an_empty_overlay() {
        let mut base = parse_table("version = 1\n", "grove.toml").unwrap();

        merge_tables(&mut base, Table::new());

        assert_eq!(base["version"].as_integer(), Some(1));
    }
}
