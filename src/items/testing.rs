use std::cell::RefCell;

use crate::AppError;
use crate::items::{ItemId, ItemStore};

/// Mock implementation of `ItemStore` for unit testing.
#[derive(Default)]
pub(crate) struct MockItemStore {
    pub(crate) add_calls: RefCell<Vec<(String, String)>>,
    pub(crate) delete_calls: RefCell<Vec<String>>,
    pub(crate) list_items_values: RefCell<Vec<String>>,
}

impl MockItemStore {
    pub(crate) fn set_list_items<I>(&self, items: I)
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        let mut values = self.list_items_values.borrow_mut();
        values.clear();
        values.extend(items.into_iter().map(Into::into));
    }
}

impl ItemStore for MockItemStore {
    fn add_item(&self, id: &ItemId, content: &str) -> Result<(), AppError> {
        self.add_calls.borrow_mut().push((id.as_str().to_string(), content.to_string()));
        Ok(())
    }

    fn list_items(&self) -> Result<Vec<String>, AppError> {
        Ok(self.list_items_values.borrow().clone())
    }

    fn delete_item(&self, id: &ItemId) -> Result<(), AppError> {
        self.delete_calls.borrow_mut().push(id.as_str().to_string());
        Ok(())
    }
}
