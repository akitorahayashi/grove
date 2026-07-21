use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::file::{self, RawConfigFile};
use crate::AppError;

#[derive(Debug)]
pub(super) struct LoadedConfigFile {
    pub path: PathBuf,
    pub directory: PathBuf,
    pub raw: RawConfigFile,
}

pub(super) struct LoadedConfigTree {
    pub root_path: PathBuf,
    pub root_directory: PathBuf,
    pub files: Vec<LoadedConfigFile>,
}

pub(super) fn load_tree(root_path: &Path) -> Result<LoadedConfigTree, AppError> {
    let root = load_one(&root_path.canonicalize()?)?;
    let root_directory = root
        .path
        .parent()
        .ok_or_else(|| AppError::config_error(format!("{} has no parent", root.path.display())))?
        .to_path_buf();
    let mut seen = HashSet::from([root.path.clone()]);
    let mut children = Vec::new();

    for include in &root.raw.include {
        let child_path = resolve_include(&root.directory, include)?;
        if !child_path.starts_with(&root_directory) {
            return Err(AppError::config_error(format!(
                "{}: include leaves the grove root",
                include
            )));
        }
        if !seen.insert(child_path.clone()) {
            return Err(AppError::config_error(format!(
                "{}: duplicate configuration file",
                child_path.display()
            )));
        }

        let child = load_one(&child_path)?;
        if !child.raw.include.is_empty() {
            return Err(AppError::config_error(format!(
                "{}: nested includes are not allowed",
                child.path.display()
            )));
        }
        children.push(child);
    }

    let root_path = root.path.clone();
    let mut files = Vec::with_capacity(children.len() + 1);
    files.push(root);
    files.extend(children);

    Ok(LoadedConfigTree { root_path, root_directory, files })
}

fn load_one(path: &Path) -> Result<LoadedConfigFile, AppError> {
    let directory = path
        .parent()
        .ok_or_else(|| AppError::config_error(format!("{} has no parent", path.display())))?
        .to_path_buf();
    let contents = fs::read_to_string(path)?;
    let raw = file::parse(&contents, &path.display().to_string())?;
    Ok(LoadedConfigFile { path: path.to_path_buf(), directory, raw })
}

fn resolve_include(base: &Path, include: &str) -> Result<PathBuf, AppError> {
    let include_path = Path::new(include);
    if include_path.is_absolute() {
        return Err(AppError::config_error(format!("{include}: include paths must be relative")));
    }

    let candidate = base.join(include_path);
    if !candidate.is_file() {
        return Err(AppError::config_error(format!(
            "{}: include target does not exist",
            candidate.display()
        )));
    }
    candidate.canonicalize().map_err(AppError::from)
}
