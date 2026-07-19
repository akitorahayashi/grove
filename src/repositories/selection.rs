use std::collections::{HashMap, HashSet};

use super::{RepositoryDefinition, RepositoryName};
use crate::AppError;

pub fn select_repositories<'a>(
    repositories: &'a [RepositoryDefinition],
    targets: &[String],
) -> Result<Vec<&'a RepositoryDefinition>, AppError> {
    if targets.is_empty() {
        return Ok(repositories.iter().collect());
    }

    let by_name: HashMap<&str, &RepositoryDefinition> =
        repositories.iter().map(|repo| (repo.name().as_str(), repo)).collect();
    let mut selected = Vec::new();
    let mut seen = HashSet::new();

    for target in targets {
        let name = RepositoryName::new(target)?;
        if !seen.insert(name.as_str().to_string()) {
            continue;
        }

        let repository = by_name
            .get(name.as_str())
            .copied()
            .ok_or_else(|| AppError::RepositoryNotFound(name.as_str().to_string()))?;
        selected.push(repository);
    }

    Ok(selected)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn repo(name: &str) -> RepositoryDefinition {
        RepositoryDefinition::new(
            RepositoryName::new(name).unwrap(),
            PathBuf::from(format!("/workspace/{name}")),
            name.to_string(),
            format!("git@example.com:{name}.git"),
            None,
            PathBuf::from("/workspace/grove.toml"),
        )
    }

    #[test]
    fn empty_targets_select_all_repositories() {
        let repositories = vec![repo("first"), repo("second")];

        let selected = select_repositories(&repositories, &[]).unwrap();

        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn explicit_targets_keep_target_order() {
        let repositories = vec![repo("first"), repo("second")];

        let selected =
            select_repositories(&repositories, &["second".to_string(), "first".to_string()])
                .unwrap();

        let names: Vec<_> = selected.iter().map(|repo| repo.name().as_str()).collect();
        assert_eq!(names, ["second", "first"]);
    }

    #[test]
    fn missing_target_fails() {
        let repositories = vec![repo("first")];

        let result = select_repositories(&repositories, &["missing".to_string()]);

        assert!(matches!(result, Err(AppError::RepositoryNotFound(ref name)) if name == "missing"));
    }
}
