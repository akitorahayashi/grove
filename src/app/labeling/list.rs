use crate::AppError;
use crate::app::AppContext;
use crate::items::{ItemId, ItemStore};
use crate::labels::LabelStore;

/// List labels attached to an item.
pub fn execute(
    ctx: &AppContext<impl ItemStore, impl LabelStore>,
    item_id: &str,
) -> Result<Vec<String>, AppError> {
    let item_id = ItemId::new(item_id)?;

    super::ensure_item_exists(ctx, &item_id)?;

    ctx.label_store().labels_for_item(item_id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::MockItemStore;
    use crate::labels::MockLabelStore;

    #[test]
    fn list_returns_labels_for_item() {
        let item_store = MockItemStore::default();
        item_store.set_list_items(["demo"]);
        let label_store = MockLabelStore::default();
        label_store.set_labels_for_item("demo", ["urgent", "backlog"]);
        let ctx = AppContext::new(item_store, label_store);

        let labels = execute(&ctx, "demo").expect("execution should succeed");
        assert_eq!(labels, vec!["urgent".to_string(), "backlog".to_string()]);
    }

    #[test]
    fn list_fails_when_item_is_missing() {
        let item_store = MockItemStore::default();
        let label_store = MockLabelStore::default();
        let ctx = AppContext::new(item_store, label_store);

        let result = execute(&ctx, "demo");
        assert!(matches!(result, Err(AppError::ItemNotFound(ref s)) if s == "demo"));
    }
}
