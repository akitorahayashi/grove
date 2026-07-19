use crate::AppError;
use crate::app::AppContext;
use crate::items::{ItemId, ItemStore};
use crate::labels::LabelStore;

/// Delete an item from storage and detach all label links for that item.
pub fn execute(
    ctx: &AppContext<impl ItemStore, impl LabelStore>,
    id: &str,
) -> Result<(), AppError> {
    let id = ItemId::new(id)?;
    ctx.item_store().delete_item(&id)?;
    ctx.label_store().detach_item(id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::MockItemStore;
    use crate::labels::MockLabelStore;

    #[test]
    fn delete_item_forwards_to_stores() {
        let item_store = MockItemStore::default();
        let label_store = MockLabelStore::default();
        let ctx = AppContext::new(item_store, label_store);

        execute(&ctx, "demo").expect("execution should succeed");

        let delete_calls = ctx.item_store().delete_calls.borrow();
        assert_eq!(delete_calls.as_slice(), ["demo".to_string()]);

        let detach_item_calls = ctx.label_store().detach_item_calls.borrow();
        assert_eq!(detach_item_calls.as_slice(), ["demo".to_string()]);
    }

    #[test]
    fn delete_item_rejects_invalid_id() {
        let item_store = MockItemStore::default();
        let label_store = MockLabelStore::default();
        let ctx = AppContext::new(item_store, label_store);

        let result = execute(&ctx, "invalid/id");
        assert!(result.is_err());
    }
}
