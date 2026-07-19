use crate::AppError;
use crate::app::AppContext;
use crate::items::ItemStore;
use crate::labels::{LabelName, LabelStore};

/// Delete a label from storage.
pub fn execute(
    ctx: &AppContext<impl ItemStore, impl LabelStore>,
    name: &str,
) -> Result<(), AppError> {
    let name = LabelName::new(name)?;
    ctx.label_store().delete_label(&name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::MockItemStore;
    use crate::labels::MockLabelStore;

    #[test]
    fn delete_label_forwards_to_store() {
        let item_store = MockItemStore::default();
        let label_store = MockLabelStore::default();
        let ctx = AppContext::new(item_store, label_store);

        execute(&ctx, "urgent").expect("execution should succeed");

        let calls = ctx.label_store().delete_calls.borrow();
        assert_eq!(calls.as_slice(), ["urgent".to_string()]);
    }

    #[test]
    fn delete_label_rejects_invalid_name() {
        let item_store = MockItemStore::default();
        let label_store = MockLabelStore::default();
        let ctx = AppContext::new(item_store, label_store);

        let result = execute(&ctx, "invalid/name");
        assert!(result.is_err());
    }
}
