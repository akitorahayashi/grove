use crate::AppError;
use crate::app::AppContext;
use crate::items::{ItemId, ItemStore};
use crate::labels::LabelStore;

/// Add an item to storage.
pub fn execute(
    ctx: &AppContext<impl ItemStore, impl LabelStore>,
    id: &str,
    content: &str,
) -> Result<(), AppError> {
    let id = ItemId::new(id)?;
    ctx.item_store().add_item(&id, content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::MockItemStore;
    use crate::labels::MockLabelStore;

    #[test]
    fn add_item_forwards_to_store() {
        let item_store = MockItemStore::default();
        let label_store = MockLabelStore::default();
        let ctx = AppContext::new(item_store, label_store);

        execute(&ctx, "demo", "example").expect("execution should succeed");

        let calls = ctx.item_store().add_calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], ("demo".to_string(), "example".to_string()));
    }

    #[test]
    fn add_item_rejects_invalid_id() {
        let item_store = MockItemStore::default();
        let label_store = MockLabelStore::default();
        let ctx = AppContext::new(item_store, label_store);

        let result = execute(&ctx, "invalid/id", "example");
        assert!(result.is_err());
    }
}
