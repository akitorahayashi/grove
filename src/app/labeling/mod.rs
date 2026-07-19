pub mod attach;
pub mod detach;
pub mod find;
pub mod list;

use crate::AppError;
use crate::app::AppContext;
use crate::items::{ItemId, ItemStore};
use crate::labels::{LabelName, LabelStore};

pub(super) fn ensure_item_exists(
    ctx: &AppContext<impl ItemStore, impl LabelStore>,
    item_id: &ItemId,
) -> Result<(), AppError> {
    if ctx.item_store().item_exists(item_id)? {
        Ok(())
    } else {
        Err(AppError::ItemNotFound(item_id.as_str().to_string()))
    }
}

pub(super) fn ensure_label_exists(
    ctx: &AppContext<impl ItemStore, impl LabelStore>,
    label_name: &LabelName,
) -> Result<(), AppError> {
    if ctx.label_store().label_exists(label_name)? {
        Ok(())
    } else {
        Err(AppError::LabelNotFound(label_name.as_str().to_string()))
    }
}
