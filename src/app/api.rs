//! Public application API facade.

use crate::AppError;
use crate::app::{AppContext, items, labeling, labels};
use crate::items::FilesystemItemStore;
use crate::labels::FilesystemLabelStore;

/// Create the default application context.
fn default_context() -> Result<AppContext<FilesystemItemStore, FilesystemLabelStore>, AppError> {
    let item_store = FilesystemItemStore::from_env()?;
    let label_store = FilesystemLabelStore::from_env()?;
    Ok(AppContext::new(item_store, label_store))
}

/// Add a new item to storage using the default backend.
pub fn item_add(id: &str, content: &str) -> Result<(), AppError> {
    let ctx = default_context()?;
    items::add::execute(&ctx, id, content)
}

/// List all stored item identifiers.
pub fn item_list() -> Result<Vec<String>, AppError> {
    let ctx = default_context()?;
    items::list::execute(&ctx)
}

/// Delete an item from storage using the default backend.
pub fn item_delete(id: &str) -> Result<(), AppError> {
    let ctx = default_context()?;
    items::delete::execute(&ctx, id)
}

/// Add a new label to storage using the default backend.
pub fn label_add(name: &str) -> Result<(), AppError> {
    let ctx = default_context()?;
    labels::add::execute(&ctx, name)
}

/// List all stored labels.
pub fn label_list() -> Result<Vec<String>, AppError> {
    let ctx = default_context()?;
    labels::list::execute(&ctx)
}

/// Delete a label from storage using the default backend.
pub fn label_delete(name: &str) -> Result<(), AppError> {
    let ctx = default_context()?;
    labels::delete::execute(&ctx, name)
}

/// Attach an existing label to an existing item.
pub fn labeling_attach(item_id: &str, label_name: &str) -> Result<(), AppError> {
    let ctx = default_context()?;
    labeling::attach::execute(&ctx, item_id, label_name)
}

/// Detach a label from an item.
pub fn labeling_detach(item_id: &str, label_name: &str) -> Result<(), AppError> {
    let ctx = default_context()?;
    labeling::detach::execute(&ctx, item_id, label_name)
}

/// List labels attached to an item.
pub fn labeling_list(item_id: &str) -> Result<Vec<String>, AppError> {
    let ctx = default_context()?;
    labeling::list::execute(&ctx, item_id)
}

/// Find items that have the given label.
pub fn labeling_find(label_name: &str) -> Result<Vec<String>, AppError> {
    let ctx = default_context()?;
    labeling::find::execute(&ctx, label_name)
}
