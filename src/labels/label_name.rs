use std::path::{Component, Path};

use crate::AppError;

/// A validated label name.
///
/// Guarantees:
/// - Non-empty
/// - Contains only alphanumeric characters or `-`
/// - No path traversal components
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelName(String);

impl LabelName {
    /// Validate and create a new `LabelName`.
    pub fn new(name: &str) -> Result<Self, AppError> {
        if Self::is_valid(name) {
            Ok(Self(name.to_string()))
        } else {
            Err(AppError::InvalidLabelName(name.to_string()))
        }
    }

    /// Return the inner string value.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn is_valid(name: &str) -> bool {
        !name.is_empty()
            && name.chars().all(|c| c.is_alphanumeric() || c == '-')
            && Path::new(name).components().all(|c| matches!(c, Component::Normal(_)))
    }
}

impl AsRef<str> for LabelName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_alphanumeric_name() {
        assert!(LabelName::new("demo123").is_ok());
    }

    #[test]
    fn valid_name_with_dashes() {
        assert!(LabelName::new("priority-high").is_ok());
    }

    #[test]
    fn empty_name_is_invalid() {
        assert!(LabelName::new("").is_err());
    }

    #[test]
    fn slash_in_name_is_invalid() {
        assert!(LabelName::new("invalid/name").is_err());
    }

    #[test]
    fn dot_dot_is_invalid() {
        assert!(LabelName::new("..").is_err());
    }

    #[test]
    fn space_in_name_is_invalid() {
        assert!(LabelName::new("has space").is_err());
    }
}
