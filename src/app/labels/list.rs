use crate::AppError;
use crate::app::AppContext;
use crate::items::ItemStore;
use crate::labels::LabelStore;

/// List all labels from storage.
pub fn execute(ctx: &AppContext<impl ItemStore, impl LabelStore>) -> Result<Vec<String>, AppError> {
    ctx.label_store().list_labels()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::MockItemStore;
    use crate::labels::MockLabelStore;

    #[test]
    fn list_labels_returns_store_values() {
        let item_store = MockItemStore::default();
        let label_store = MockLabelStore::default();
        label_store.set_list_labels(["urgent", "backlog"]);
        let ctx = AppContext::new(item_store, label_store);

        let labels = execute(&ctx).expect("execution should succeed");
        assert_eq!(labels, vec!["urgent".to_string(), "backlog".to_string()]);
    }
}
