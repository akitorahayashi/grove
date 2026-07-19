use std::path::{Component, Path};

use serde::Serialize;

use crate::AppError;

/// A validated repository name used for CLI target selection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct RepositoryName(String);

impl RepositoryName {
    pub fn new(name: &str) -> Result<Self, AppError> {
        if Self::is_valid(name) {
            Ok(Self(name.to_string()))
        } else {
            Err(AppError::InvalidRepositoryName(name.to_string()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn is_valid(name: &str) -> bool {
        !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            && Path::new(name).components().all(|c| matches!(c, Component::Normal(_)))
    }
}

impl AsRef<str> for RepositoryName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for RepositoryName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_common_repository_names() {
        assert!(RepositoryName::new("frontend").is_ok());
        assert!(RepositoryName::new("frontend-api").is_ok());
        assert!(RepositoryName::new("company.service").is_ok());
        assert!(RepositoryName::new("tool_v2").is_ok());
    }

    #[test]
    fn rejects_path_like_names() {
        assert!(RepositoryName::new("").is_err());
        assert!(RepositoryName::new("../repo").is_err());
        assert!(RepositoryName::new("team/repo").is_err());
        assert!(RepositoryName::new("has space").is_err());
    }
}
