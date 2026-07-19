use serde::Deserialize;

use crate::AppError;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawConfigFile {
    pub version: Option<u32>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default, rename = "repo")]
    pub repositories: Vec<RawRepository>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawRepository {
    pub name: Option<String>,
    pub path: Option<String>,
    pub url: Option<String>,
    pub default_branch: Option<String>,
}

pub(super) fn parse(contents: &str, label: &str) -> Result<RawConfigFile, AppError> {
    toml::from_str(contents)
        .map_err(|err| AppError::config_error(format!("{label}: invalid TOML: {err}")))
}
