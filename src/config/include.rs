use std::collections::HashSet;
use std::fs;
use std::io;
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
    let label = path.display().to_string();
    let contents = fs::read_to_string(path)?;
    let mut table = file::parse_table(&contents, &label)?;

    if let Some((override_path, override_contents)) = read_sibling_override(path)? {
        let override_table =
            file::parse_table(&override_contents, &override_path.display().to_string())?;
        file::merge_tables(&mut table, override_table);
    }

    let raw = file::decode(table, &label)?;
    Ok(LoadedConfigFile { path: path.to_path_buf(), directory, raw })
}

/// The sibling override file for `grove.toml` is `grove.override.toml`; for an
/// arbitrarily named `--config`/include target `custom.toml` it is
/// `custom.override.toml`. Naming it from the loaded file's stem, rather than
/// hardcoding `grove.override.toml`, keeps `--config` and `include` targets
/// overridable the same way the default file is.
fn sibling_override_path(path: &Path) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default();
    let file_name = match path.extension() {
        Some(extension) => {
            format!("{}.override.{}", stem.to_string_lossy(), extension.to_string_lossy())
        }
        None => format!("{}.override", stem.to_string_lossy()),
    };
    path.with_file_name(file_name)
}

/// Mirrors `discovery::locate`'s symlink handling: a missing override is a
/// normal, silent absence, but a broken symlink, a non-regular file, or a
/// permission failure must surface rather than be treated as "no override" and
/// silently ignored.
fn read_sibling_override(path: &Path) -> Result<Option<(PathBuf, String)>, AppError> {
    let override_path = sibling_override_path(path);
    match fs::symlink_metadata(&override_path) {
        Ok(_) => {
            let resolved = override_path.canonicalize().map_err(|err| {
                AppError::config_source(format!("{}: {err}", override_path.display()), err)
            })?;
            if !resolved.is_file() {
                return Err(AppError::config_error(format!(
                    "{}: not a regular file",
                    override_path.display()
                )));
            }
            let contents = fs::read_to_string(&resolved)?;
            Ok(Some((resolved, contents)))
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => {
            Err(AppError::config_source(format!("{}: {err}", override_path.display()), err))
        }
    }
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
