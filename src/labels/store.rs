use super::label_name::LabelName;
use crate::AppError;

/// Storage contract for label and labeling operations.
pub trait LabelStore {
    /// Store a label with the given name.
    fn add_label(&self, name: &LabelName) -> Result<(), AppError>;

    /// List all stored labels.
    fn list_labels(&self) -> Result<Vec<String>, AppError>;

    /// Return whether one label exists.
    fn label_exists(&self, name: &LabelName) -> Result<bool, AppError> {
        Ok(self.list_labels()?.iter().any(|existing| existing == name.as_str()))
    }

    /// Remove a label by name.
    fn delete_label(&self, name: &LabelName) -> Result<(), AppError>;

    /// Attach a label to an item.
    fn attach_label(&self, item_id: &str, label_name: &LabelName) -> Result<(), AppError>;

    /// Detach a label from an item.
    fn detach_label(&self, item_id: &str, label_name: &LabelName) -> Result<(), AppError>;

    /// List labels attached to one item.
    fn labels_for_item(&self, item_id: &str) -> Result<Vec<String>, AppError>;

    /// List items attached to one label.
    fn items_for_label(&self, label_name: &LabelName) -> Result<Vec<String>, AppError>;

    /// Remove all label links for one item.
    fn detach_item(&self, item_id: &str) -> Result<(), AppError>;
}
