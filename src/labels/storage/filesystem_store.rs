use std::fs;
use std::path::{Path, PathBuf};

use super::LabelStorageSettings;
use crate::AppError;
use crate::labels::{LabelName, LabelStore};

/// Filesystem-based implementation of `LabelStore`.
#[derive(Debug, Clone)]
pub struct FilesystemLabelStore {
    root_path: PathBuf,
}

impl FilesystemLabelStore {
    /// Create a new store with the given settings.
    pub fn new(settings: &LabelStorageSettings) -> Self {
        Self { root_path: settings.storage_path.clone() }
    }

    /// Create a store with default environment-based settings.
    pub fn from_env() -> Result<Self, AppError> {
        let settings = LabelStorageSettings::from_env()?;
        Ok(Self::new(&settings))
    }

    fn definitions_root(&self) -> PathBuf {
        self.root_path.join("definitions")
    }

    fn links_root(&self) -> PathBuf {
        self.root_path.join("links")
    }

    fn label_dir(&self, name: &LabelName) -> PathBuf {
        self.definitions_root().join(name.as_str())
    }

    fn label_file(&self, name: &LabelName) -> PathBuf {
        self.label_dir(name).join("label.txt")
    }

    fn item_links_dir(&self, item_id: &str) -> PathBuf {
        self.links_root().join(item_id)
    }

    fn item_label_link(&self, item_id: &str, label_name: &LabelName) -> PathBuf {
        self.item_links_dir(item_id).join(label_name.as_str())
    }

    fn label_definition_exists(&self, label_name: &LabelName) -> bool {
        self.label_dir(label_name).exists()
    }

    fn cleanup_empty_dir(path: &Path) -> Result<(), AppError> {
        if !path.exists() {
            return Ok(());
        }

        let mut entries = fs::read_dir(path)?;
        if entries.next().is_none() {
            fs::remove_dir(path)?;
        }

        Ok(())
    }

    fn valid_item_key(item_id: &str) -> bool {
        !item_id.is_empty() && item_id.chars().all(|c| c.is_alphanumeric() || c == '-')
    }

    fn validate_item_key(item_id: &str) -> Result<(), AppError> {
        if Self::valid_item_key(item_id) {
            Ok(())
        } else {
            Err(AppError::InvalidItemId(item_id.to_string()))
        }
    }
}

impl LabelStore for FilesystemLabelStore {
    fn add_label(&self, name: &LabelName) -> Result<(), AppError> {
        let directory = self.label_dir(name);
        fs::create_dir_all(&directory)?;
        fs::write(self.label_file(name), name.as_str())?;
        Ok(())
    }

    fn list_labels(&self) -> Result<Vec<String>, AppError> {
        let definitions_root = self.definitions_root();
        if !definitions_root.exists() {
            return Ok(Vec::new());
        }

        let mut names = Vec::new();
        for entry in fs::read_dir(definitions_root)? {
            let entry = entry?;
            if entry.path().is_dir()
                && let Some(name) = entry.file_name().to_str()
                && LabelName::new(name).is_ok()
            {
                names.push(name.to_string());
            }
        }

        names.sort();
        Ok(names)
    }

    fn label_exists(&self, name: &LabelName) -> Result<bool, AppError> {
        Ok(self.label_definition_exists(name))
    }

    fn delete_label(&self, name: &LabelName) -> Result<(), AppError> {
        let directory = self.label_dir(name);
        if !directory.exists() {
            return Err(AppError::LabelNotFound(name.as_str().to_string()));
        }

        fs::remove_dir_all(&directory)?;

        let links_root = self.links_root();
        if links_root.exists() {
            for entry in fs::read_dir(&links_root)? {
                let entry = entry?;
                let item_links = entry.path();
                if !item_links.is_dir() {
                    continue;
                }

                let link = item_links.join(name.as_str());
                if link.exists() {
                    fs::remove_file(link)?;
                }

                Self::cleanup_empty_dir(&item_links)?;
            }

            Self::cleanup_empty_dir(&links_root)?;
        }

        Ok(())
    }

    fn attach_label(&self, item_id: &str, label_name: &LabelName) -> Result<(), AppError> {
        Self::validate_item_key(item_id)?;

        if !self.label_definition_exists(label_name) {
            return Err(AppError::LabelNotFound(label_name.as_str().to_string()));
        }

        let item_links = self.item_links_dir(item_id);
        fs::create_dir_all(&item_links)?;
        fs::write(self.item_label_link(item_id, label_name), b"")?;
        Ok(())
    }

    fn detach_label(&self, item_id: &str, label_name: &LabelName) -> Result<(), AppError> {
        Self::validate_item_key(item_id)?;

        let link = self.item_label_link(item_id, label_name);
        if !link.exists() {
            return Err(AppError::LabelingNotFound {
                item_id: item_id.to_string(),
                label_name: label_name.as_str().to_string(),
            });
        }

        fs::remove_file(&link)?;
        let item_links = self.item_links_dir(item_id);
        Self::cleanup_empty_dir(&item_links)?;
        let links_root = self.links_root();
        Self::cleanup_empty_dir(&links_root)?;
        Ok(())
    }

    fn labels_for_item(&self, item_id: &str) -> Result<Vec<String>, AppError> {
        Self::validate_item_key(item_id)?;

        let item_links = self.item_links_dir(item_id);
        if !item_links.exists() {
            return Ok(Vec::new());
        }

        let mut labels = Vec::new();
        for entry in fs::read_dir(item_links)? {
            let entry = entry?;
            if entry.path().is_file()
                && let Some(name) = entry.file_name().to_str()
                && LabelName::new(name).is_ok()
            {
                labels.push(name.to_string());
            }
        }

        labels.sort();
        Ok(labels)
    }

    fn items_for_label(&self, label_name: &LabelName) -> Result<Vec<String>, AppError> {
        if !self.label_definition_exists(label_name) {
            return Err(AppError::LabelNotFound(label_name.as_str().to_string()));
        }

        let links_root = self.links_root();
        if !links_root.exists() {
            return Ok(Vec::new());
        }

        let mut item_ids = Vec::new();
        for entry in fs::read_dir(links_root)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let file_name = entry.file_name();
            let Some(item_id) = file_name.to_str() else {
                continue;
            };
            if !Self::valid_item_key(item_id) {
                continue;
            }

            if path.join(label_name.as_str()).exists() {
                item_ids.push(item_id.to_string());
            }
        }

        item_ids.sort();
        Ok(item_ids)
    }

    fn detach_item(&self, item_id: &str) -> Result<(), AppError> {
        Self::validate_item_key(item_id)?;

        let item_links = self.item_links_dir(item_id);
        if item_links.exists() {
            fs::remove_dir_all(item_links)?;
        }

        let links_root = self.links_root();
        Self::cleanup_empty_dir(&links_root)?;
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

        fn store(&self) -> FilesystemLabelStore {
            let settings = LabelStorageSettings::with_path(self.storage_root());
            FilesystemLabelStore::new(&settings)
        }

        fn storage_root(&self) -> PathBuf {
            self.root.path().join(".config").join("rs-cli-tmpl").join("labels")
        }
    }

    #[test]
    fn add_label_persists_definition() {
        let ctx = TestContext::new();
        let store = ctx.store();
        let name = LabelName::new("urgent").unwrap();

        store.add_label(&name).expect("add_label should succeed");

        let saved = ctx.storage_root().join("definitions").join("urgent").join("label.txt");
        let content = fs::read_to_string(saved).expect("failed to read saved label");
        assert_eq!(content, "urgent");
    }

    #[test]
    fn list_labels_returns_sorted_values() {
        let ctx = TestContext::new();
        let store = ctx.store();

        store.add_label(&LabelName::new("second").unwrap()).unwrap();
        store.add_label(&LabelName::new("first").unwrap()).unwrap();

        let labels = store.list_labels().expect("list_labels should succeed");
        assert_eq!(labels, vec!["first", "second"]);
    }

    #[test]
    fn delete_label_removes_definition_and_links() {
        let ctx = TestContext::new();
        let store = ctx.store();
        let label = LabelName::new("cleanup").unwrap();

        store.add_label(&label).unwrap();
        store.attach_label("item-a", &label).unwrap();

        store.delete_label(&label).expect("delete_label should succeed");

        assert!(!ctx.storage_root().join("definitions").join("cleanup").exists());
        assert!(!ctx.storage_root().join("links").join("item-a").join("cleanup").exists());
    }

    #[test]
    fn attach_and_query_links_work() {
        let ctx = TestContext::new();
        let store = ctx.store();
        let urgent = LabelName::new("urgent").unwrap();
        let backlog = LabelName::new("backlog").unwrap();

        store.add_label(&urgent).unwrap();
        store.add_label(&backlog).unwrap();
        store.attach_label("item-a", &urgent).unwrap();
        store.attach_label("item-b", &urgent).unwrap();
        store.attach_label("item-b", &backlog).unwrap();

        let labels = store.labels_for_item("item-b").expect("labels_for_item should succeed");
        assert_eq!(labels, vec!["backlog", "urgent"]);

        let items = store.items_for_label(&urgent).expect("items_for_label should succeed");
        assert_eq!(items, vec!["item-a", "item-b"]);
    }

    #[test]
    fn attach_fails_if_label_is_missing() {
        let ctx = TestContext::new();
        let store = ctx.store();
        let label = LabelName::new("missing").unwrap();

        let result = store.attach_label("item-a", &label);
        assert!(matches!(result, Err(AppError::LabelNotFound(ref s)) if s == "missing"));
    }

    #[test]
    fn attach_fails_if_item_id_is_invalid() {
        let ctx = TestContext::new();
        let store = ctx.store();
        let label = LabelName::new("urgent").unwrap();
        store.add_label(&label).unwrap();

        let result = store.attach_label("invalid/id", &label);
        assert!(matches!(result, Err(AppError::InvalidItemId(ref s)) if s == "invalid/id"));
    }

    #[test]
    fn detach_fails_if_link_is_missing() {
        let ctx = TestContext::new();
        let store = ctx.store();
        let label = LabelName::new("urgent").unwrap();

        store.add_label(&label).unwrap();

        let result = store.detach_label("item-a", &label);
        assert!(matches!(
            result,
            Err(AppError::LabelingNotFound { ref item_id, ref label_name })
                if item_id == "item-a" && label_name == "urgent"
        ));
    }

    #[test]
    fn detach_item_removes_all_links() {
        let ctx = TestContext::new();
        let store = ctx.store();
        let label = LabelName::new("urgent").unwrap();

        store.add_label(&label).unwrap();
        store.attach_label("item-a", &label).unwrap();

        store.detach_item("item-a").expect("detach_item should succeed");

        let labels = store.labels_for_item("item-a").expect("labels_for_item should succeed");
        assert!(labels.is_empty());
    }

    #[test]
    fn labels_for_item_fails_if_item_id_is_invalid() {
        let ctx = TestContext::new();
        let store = ctx.store();

        let result = store.labels_for_item("invalid/id");
        assert!(matches!(result, Err(AppError::InvalidItemId(ref s)) if s == "invalid/id"));
    }

    #[test]
    fn detach_item_fails_if_item_id_is_invalid() {
        let ctx = TestContext::new();
        let store = ctx.store();

        let result = store.detach_item("invalid/id");
        assert!(matches!(result, Err(AppError::InvalidItemId(ref s)) if s == "invalid/id"));
    }
}
