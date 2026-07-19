use super::item_id::ItemId;
use crate::AppError;

/// Storage contract for item operations.
pub trait ItemStore {
    /// Store an item with the given identifier and content.
    fn add_item(&self, id: &ItemId, content: &str) -> Result<(), AppError>;

    /// List all stored item identifiers.
    fn list_items(&self) -> Result<Vec<String>, AppError>;

    /// Return whether one item exists.
    fn item_exists(&self, id: &ItemId) -> Result<bool, AppError> {
        Ok(self.list_items()?.iter().any(|existing| existing == id.as_str()))
    }

    /// Remove an item by identifier.
    fn delete_item(&self, id: &ItemId) -> Result<(), AppError>;
}
