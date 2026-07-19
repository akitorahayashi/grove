use std::cell::RefCell;
use std::collections::HashMap;

use crate::AppError;
use crate::labels::{LabelName, LabelStore};

/// Mock implementation of `LabelStore` for unit testing.
#[derive(Default)]
pub(crate) struct MockLabelStore {
    pub(crate) add_calls: RefCell<Vec<String>>,
    pub(crate) delete_calls: RefCell<Vec<String>>,
    pub(crate) attach_calls: RefCell<Vec<(String, String)>>,
    pub(crate) detach_calls: RefCell<Vec<(String, String)>>,
    pub(crate) detach_item_calls: RefCell<Vec<String>>,
    pub(crate) list_labels_values: RefCell<Vec<String>>,
    labels_for_item_values: RefCell<HashMap<String, Vec<String>>>,
    items_for_label_values: RefCell<HashMap<String, Vec<String>>>,
}

impl MockLabelStore {
    pub(crate) fn set_list_labels<I>(&self, labels: I)
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        let mut values = self.list_labels_values.borrow_mut();
        values.clear();
        values.extend(labels.into_iter().map(Into::into));
    }

    pub(crate) fn set_labels_for_item<I>(&self, item_id: &str, labels: I)
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        let values = labels.into_iter().map(Into::into).collect::<Vec<String>>();
        self.labels_for_item_values.borrow_mut().insert(item_id.to_string(), values);
    }

    pub(crate) fn set_items_for_label<I>(&self, label_name: &str, items: I)
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        let values = items.into_iter().map(Into::into).collect::<Vec<String>>();
        self.items_for_label_values.borrow_mut().insert(label_name.to_string(), values);
    }
}

impl LabelStore for MockLabelStore {
    fn add_label(&self, name: &LabelName) -> Result<(), AppError> {
        self.add_calls.borrow_mut().push(name.as_str().to_string());
        Ok(())
    }

    fn list_labels(&self) -> Result<Vec<String>, AppError> {
        Ok(self.list_labels_values.borrow().clone())
    }

    fn delete_label(&self, name: &LabelName) -> Result<(), AppError> {
        self.delete_calls.borrow_mut().push(name.as_str().to_string());
        Ok(())
    }

    fn attach_label(&self, item_id: &str, label_name: &LabelName) -> Result<(), AppError> {
        self.attach_calls.borrow_mut().push((item_id.to_string(), label_name.as_str().to_string()));
        Ok(())
    }

    fn detach_label(&self, item_id: &str, label_name: &LabelName) -> Result<(), AppError> {
        self.detach_calls.borrow_mut().push((item_id.to_string(), label_name.as_str().to_string()));
        Ok(())
    }

    fn labels_for_item(&self, item_id: &str) -> Result<Vec<String>, AppError> {
        Ok(self.labels_for_item_values.borrow().get(item_id).cloned().unwrap_or_default())
    }

    fn items_for_label(&self, label_name: &LabelName) -> Result<Vec<String>, AppError> {
        Ok(self
            .items_for_label_values
            .borrow()
            .get(label_name.as_str())
            .cloned()
            .unwrap_or_default())
    }

    fn detach_item(&self, item_id: &str) -> Result<(), AppError> {
        self.detach_item_calls.borrow_mut().push(item_id.to_string());
        Ok(())
    }
}
