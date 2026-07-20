use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Debug)]
pub(crate) enum ResolutionError {
    Io(io::Error),
    OutsideRoot,
}

impl From<io::Error> for ResolutionError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) fn resolve_operational_path(
    candidate: &Path,
    canonical_root: &Path,
) -> Result<PathBuf, ResolutionError> {
    let mut ancestor = candidate;
    let mut suffix = Vec::new();

    while !ancestor.try_exists()? {
        let component = ancestor.file_name().ok_or_else(|| {
            ResolutionError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("path '{}' has no existing ancestor", candidate.display()),
            ))
        })?;
        suffix.push(component.to_os_string());
        ancestor = ancestor.parent().ok_or_else(|| {
            ResolutionError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("path '{}' has no existing ancestor", candidate.display()),
            ))
        })?;
    }

    let mut resolved = ancestor.canonicalize()?;
    for component in suffix.iter().rev() {
        resolved.push(component);
    }

    if resolved.starts_with(canonical_root) {
        Ok(resolved)
    } else {
        Err(ResolutionError::OutsideRoot)
    }
}

pub(crate) fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(std::path::MAIN_SEPARATOR.to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}
