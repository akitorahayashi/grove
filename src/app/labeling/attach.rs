use crate::AppError;
use crate::app::AppContext;
use crate::items::{ItemId, ItemStore};
use crate::labels::{LabelName, LabelStore};

/// Attach a label to an item.
pub fn execute(
    ctx: &AppContext<impl ItemStore, impl LabelStore>,
    item_id: &str,
    label_name: &str,
) -> Result<(), AppError> {
    let item_id = ItemId::new(item_id)?;
    let label_name = LabelName::new(label_name)?;

    super::ensure_item_exists(ctx, &item_id)?;
    super::ensure_label_exists(ctx, &label_name)?;

    ctx.label_store().attach_label(item_id.as_str(), &label_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::MockItemStore;
    use crate::labels::MockLabelStore;

    #[test]
    fn attach_forwards_to_store_when_inputs_exist() {
        let item_store = MockItemStore::default();
        item_store.set_list_items(["demo"]);
        let label_store = MockLabelStore::default();
        label_store.set_list_labels(["urgent"]);
        let ctx = AppContext::new(item_store, label_store);

        execute(&ctx, "demo", "urgent").expect("execution should succeed");

        let calls = ctx.label_store().attach_calls.borrow();
        assert_eq!(calls.as_slice(), [("demo".to_string(), "urgent".to_string())]);
    }

    #[test]
    fn attach_fails_when_item_is_missing() {
        let item_store = MockItemStore::default();
        let label_store = MockLabelStore::default();
        label_store.set_list_labels(["urgent"]);
        let ctx = AppContext::new(item_store, label_store);

        let result = execute(&ctx, "demo", "urgent");
        assert!(matches!(result, Err(AppError::ItemNotFound(ref s)) if s == "demo"));
    }

    #[test]
    fn attach_fails_when_label_is_missing() {
        let item_store = MockItemStore::default();
        item_store.set_list_items(["demo"]);
        let label_store = MockLabelStore::default();
        let ctx = AppContext::new(item_store, label_store);

        let result = execute(&ctx, "demo", "urgent");
        assert!(matches!(result, Err(AppError::LabelNotFound(ref s)) if s == "urgent"));
    }

    #[test]
    fn attach_rejects_invalid_item_id() {
        let item_store = MockItemStore::default();
        let label_store = MockLabelStore::default();
        let ctx = AppContext::new(item_store, label_store);

        let result = execute(&ctx, "invalid/id", "urgent");
        assert!(matches!(result, Err(AppError::InvalidItemId(_))));
    }

    #[test]
    fn attach_rejects_invalid_label_name() {
        let item_store = MockItemStore::default();
        item_store.set_list_items(["demo"]);
        let label_store = MockLabelStore::default();
        let ctx = AppContext::new(item_store, label_store);

        let result = execute(&ctx, "demo", "invalid/label");
        assert!(matches!(result, Err(AppError::InvalidLabelName(_))));
    }
}
