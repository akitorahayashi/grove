use crate::items::ItemStore;
use crate::labels::LabelStore;

/// Application context holding dependencies for command execution.
pub struct AppContext<I: ItemStore, L: LabelStore> {
    item_store: I,
    label_store: L,
}

impl<I: ItemStore, L: LabelStore> AppContext<I, L> {
    /// Create a new application context with the given store.
    pub fn new(item_store: I, label_store: L) -> Self {
        Self { item_store, label_store }
    }

    /// Get a reference to the item store.
    pub fn item_store(&self) -> &I {
        &self.item_store
    }

    /// Get a reference to the label store.
    pub fn label_store(&self) -> &L {
        &self.label_store
    }
}
