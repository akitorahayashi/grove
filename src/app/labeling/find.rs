use crate::AppError;
use crate::app::AppContext;
use crate::items::ItemStore;
use crate::labels::{LabelName, LabelStore};

/// Find items that are attached to one label.
pub fn execute(
    ctx: &AppContext<impl ItemStore, impl LabelStore>,
    label_name: &str,
) -> Result<Vec<String>, AppError> {
    let label_name = LabelName::new(label_name)?;

    super::ensure_label_exists(ctx, &label_name)?;

    ctx.label_store().items_for_label(&label_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::MockItemStore;
    use crate::labels::MockLabelStore;

    #[test]
    fn find_returns_items_for_label() {
        let item_store = MockItemStore::default();
        let label_store = MockLabelStore::default();
        label_store.set_list_labels(["urgent"]);
        label_store.set_items_for_label("urgent", ["first", "second"]);
        let ctx = AppContext::new(item_store, label_store);

        let items = execute(&ctx, "urgent").expect("execution should succeed");
        assert_eq!(items, vec!["first".to_string(), "second".to_string()]);
    }

    #[test]
    fn find_fails_when_label_is_missing() {
        let item_store = MockItemStore::default();
        let label_store = MockLabelStore::default();
        let ctx = AppContext::new(item_store, label_store);

        let result = execute(&ctx, "urgent");
        assert!(matches!(result, Err(AppError::LabelNotFound(ref s)) if s == "urgent"));
    }

    #[test]
    fn find_rejects_invalid_label_name() {
        let item_store = MockItemStore::default();
        let label_store = MockLabelStore::default();
        let ctx = AppContext::new(item_store, label_store);

        let result = execute(&ctx, "invalid/name");
        assert!(matches!(result, Err(AppError::InvalidLabelName(_))));
    }
}
