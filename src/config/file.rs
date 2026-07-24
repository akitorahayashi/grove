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

pub(super) fn parse(contents: &str, label: &str) -> Result<RawConfigFile, AppError> {
    let mut root = contents
        .parse::<Table>()
        .map_err(|err| AppError::config_source(format!("{label}: invalid TOML: {err}"), err))?;

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
    use super::parse;

    #[test]
    fn preserves_repository_declaration_order() {
        let config = parse(
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

        let names = config.repositories.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>();

        assert_eq!(names, ["second", "first"]);
    }
}
