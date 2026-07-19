use std::fs;
use std::path::PathBuf;

use super::StorageSettings;
use crate::AppError;
use crate::items::{ItemId, ItemStore};

/// Filesystem-based implementation of `ItemStore`.
#[derive(Debug, Clone)]
pub struct FilesystemItemStore {
    root_path: PathBuf,
}

impl FilesystemItemStore {
    /// Create a new store with the given settings.
    pub fn new(settings: &StorageSettings) -> Self {
        Self { root_path: settings.storage_path.clone() }
    }

    /// Create a store with default environment-based settings.
    pub fn from_env() -> Result<Self, AppError> {
        let settings = StorageSettings::from_env()?;
        Ok(Self::new(&settings))
    }

    fn item_dir(&self, id: &ItemId) -> PathBuf {
        self.root_path.join(id.as_str())
    }

    fn item_file(&self, id: &ItemId) -> PathBuf {
        self.item_dir(id).join("item.txt")
    }
}

impl ItemStore for FilesystemItemStore {
    fn add_item(&self, id: &ItemId, content: &str) -> Result<(), AppError> {
        let directory = self.item_dir(id);
        fs::create_dir_all(&directory)?;
        fs::write(self.item_file(id), content)?;
        Ok(())
    }

    fn list_items(&self) -> Result<Vec<String>, AppError> {
        if !self.root_path.exists() {
            return Ok(Vec::new());
        }

        let mut ids = Vec::new();
        for entry in fs::read_dir(&self.root_path)? {
            let entry = entry?;
            if entry.path().is_dir()
                && let Some(name) = entry.file_name().to_str()
                && ItemId::new(name).is_ok()
            {
                ids.push(name.to_string());
            }
        }

        ids.sort();
        Ok(ids)
    }

    fn item_exists(&self, id: &ItemId) -> Result<bool, AppError> {
        Ok(self.item_dir(id).exists())
    }

    fn delete_item(&self, id: &ItemId) -> Result<(), AppError> {
        let directory = self.item_dir(id);
        if !directory.exists() {
            return Err(AppError::ItemNotFound(id.as_str().to_string()));
        }
        fs::remove_dir_all(directory)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct TestContext {
        root: TempDir,
    }

    impl TestContext {
        fn new() -> Self {
            let root = TempDir::new().expect("failed to create temp dir");
            Self { root }
        }

        fn store(&self) -> FilesystemItemStore {
            let settings = StorageSettings::with_path(self.storage_root());
            FilesystemItemStore::new(&settings)
        }

        fn storage_root(&self) -> PathBuf {
            self.root.path().join(".config").join("rs-cli-tmpl").join("items")
        }
    }

    #[test]
    fn add_item_persists_contents() {
        let ctx = TestContext::new();
        let store = ctx.store();
        let id = ItemId::new("demo").unwrap();

        store.add_item(&id, "example content").expect("add_item should succeed");

        let saved = ctx.storage_root().join("demo").join("item.txt");
        let content = fs::read_to_string(saved).expect("failed to read saved item");
        assert_eq!(content, "example content");
    }

    #[test]
    fn list_items_returns_all_ids() {
        let ctx = TestContext::new();
        let store = ctx.store();

        store.add_item(&ItemId::new("first").unwrap(), "one").unwrap();
        store.add_item(&ItemId::new("second").unwrap(), "two").unwrap();

        let mut items = store.list_items().expect("list_items succeeds");
        items.sort();
        assert_eq!(items, vec!["first", "second"]);
    }

    #[test]
    fn delete_item_removes_directory() {
        let ctx = TestContext::new();
        let store = ctx.store();
        let id = ItemId::new("temp").unwrap();

        store.add_item(&id, "data").unwrap();
        store.delete_item(&id).expect("delete succeeds");

        assert!(!ctx.storage_root().join("temp").exists());
    }

    #[test]
    fn delete_item_fails_if_not_exists() {
        let ctx = TestContext::new();
        let store = ctx.store();
        let id = ItemId::new("nonexistent").unwrap();

        let result = store.delete_item(&id);
        assert!(matches!(result, Err(AppError::ItemNotFound(ref s)) if s == "nonexistent"));
    }
}
