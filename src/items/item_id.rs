use std::path::{Component, Path};

use crate::AppError;

/// A validated item identifier.
///
/// Guarantees:
/// - Non-empty
/// - Contains only alphanumeric characters or `-`
/// - No path traversal components
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemId(String);

impl ItemId {
    /// Validate and create a new `ItemId`.
    pub fn new(id: &str) -> Result<Self, AppError> {
        if Self::is_valid(id) {
            Ok(Self(id.to_string()))
        } else {
            Err(AppError::InvalidItemId(id.to_string()))
        }
    }

    /// Return the inner string value.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn is_valid(id: &str) -> bool {
        !id.is_empty()
            && id.chars().all(|c| c.is_alphanumeric() || c == '-')
            && Path::new(id).components().all(|c| matches!(c, Component::Normal(_)))
    }
}

impl AsRef<str> for ItemId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_alphanumeric_id() {
        assert!(ItemId::new("demo123").is_ok());
    }

    #[test]
    fn valid_id_with_dashes() {
        assert!(ItemId::new("my-item-1").is_ok());
    }

    #[test]
    fn empty_id_is_invalid() {
        assert!(ItemId::new("").is_err());
    }

    #[test]
    fn slash_in_id_is_invalid() {
        assert!(ItemId::new("invalid/id").is_err());
    }

    #[test]
    fn dot_dot_is_invalid() {
        assert!(ItemId::new("..").is_err());
    }

    #[test]
    fn space_in_id_is_invalid() {
        assert!(ItemId::new("has space").is_err());
    }
}
