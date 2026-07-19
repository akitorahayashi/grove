use crate::AppError;
use crate::app::AppContext;
use crate::items::ItemStore;
use crate::labels::LabelStore;

/// List all item identifiers from storage.
pub fn execute(ctx: &AppContext<impl ItemStore, impl LabelStore>) -> Result<Vec<String>, AppError> {
    ctx.item_store().list_items()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::MockItemStore;
    use crate::labels::MockLabelStore;

    #[test]
    fn list_items_returns_store_values() {
        let item_store = MockItemStore::default();
        item_store.set_list_items(["first", "second"]);
        let label_store = MockLabelStore::default();
        let ctx = AppContext::new(item_store, label_store);

        let items = execute(&ctx).expect("execution should succeed");
        assert_eq!(items, vec!["first".to_string(), "second".to_string()]);
    }
}
