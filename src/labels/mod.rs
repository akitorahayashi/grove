mod label_name;
pub mod storage;
mod store;

#[cfg(test)]
mod testing;

pub use label_name::LabelName;
pub use storage::{FilesystemLabelStore, LabelStorageSettings};
pub use store::LabelStore;

#[cfg(test)]
pub(crate) use testing::MockLabelStore;
