use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use tempfile::NamedTempFile;
use toml_edit::{DocumentMut, InlineTable, Item, Table, Value, value};

use super::include::{resolve_sibling_override_path, sibling_override_path};
use super::{ResolvedConfig, load, load_with_replacement};
use crate::AppError;
use crate::repositories::{RemoteUrl, RepositoryName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Addition {
    Added { name: String, display_path: String },
    DurabilityUnconfirmed { name: String, display_path: String, message: String },
    Unchanged { name: String, display_path: String },
}

pub(crate) struct EditSession {
    root_path: PathBuf,
    root_directory: PathBuf,
    destination: PathBuf,
    document: DocumentMut,
    baseline: Option<String>,
    resolved: ResolvedConfig,
    dry_run: bool,
    _locks: Vec<DirectoryLock>,
}

impl EditSession {
    pub(crate) fn open(
        root_path: &Path,
        override_file: bool,
        dry_run: bool,
    ) -> Result<Self, AppError> {
        let root_path = root_path.canonicalize()?;
        let root_directory = root_path
            .parent()
            .ok_or_else(|| {
                AppError::config_error(format!("{} has no parent", root_path.display()))
            })?
            .to_path_buf();
        let destination = if override_file {
            resolve_sibling_override_path(&root_path)?
                .unwrap_or_else(|| sibling_override_path(&root_path))
        } else {
            root_path.clone()
        };
        let destination_directory = destination
            .parent()
            .ok_or_else(|| {
                AppError::config_error(format!("{} has no parent", destination.display()))
            })?
            .canonicalize()?;
        let mut lock_directories = vec![root_directory.clone(), destination_directory];
        lock_directories.sort();
        lock_directories.dedup();
        let locks = lock_directories
            .iter()
            .map(|directory| DirectoryLock::acquire(directory, dry_run))
            .collect::<Result<Vec<_>, _>>()?;
        let resolved = load(Some(&root_path))?;
        let baseline = match fs::read_to_string(&destination) {
            Ok(contents) => Some(contents),
            Err(error) if override_file && error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let contents = baseline.as_deref().unwrap_or_default();
        let label = destination.display().to_string();
        let document = contents.parse::<DocumentMut>().map_err(|error| {
            AppError::config_source(format!("{label}: invalid TOML: {error}"), error)
        })?;
        if document.to_string() != contents {
            return Err(AppError::config_error(format!(
                "{label}: cannot preserve this TOML formatting; add the repository manually"
            )));
        }

        Ok(Self {
            root_path,
            root_directory,
            destination,
            document,
            baseline,
            resolved,
            dry_run,
            _locks: locks,
        })
    }

    pub(crate) fn destination(&self) -> &Path {
        &self.destination
    }

    pub(crate) fn add(
        &mut self,
        name: RepositoryName,
        repository_path: &Path,
        url: RemoteUrl,
    ) -> Result<Addition, AppError> {
        let repository_path = repository_path.canonicalize()?;
        if let Some(existing) =
            self.resolved.repositories().iter().find(|entry| entry.path() == repository_path)
        {
            if existing.url().matches(&url) {
                return Ok(Addition::Unchanged {
                    name: existing.name().as_str().to_string(),
                    display_path: existing.display_path().to_string(),
                });
            }
            return Err(AppError::config_error(format!(
                "repository path '{}' is already configured as '{}' with a different URL",
                repository_path.display(),
                existing.name()
            )));
        }
        if let Some(existing) =
            self.resolved.repositories().iter().find(|entry| entry.name() == &name)
        {
            return Err(AppError::config_error(format!(
                "repository name '{}' is already configured at '{}'",
                name,
                existing.path().display()
            )));
        }

        let display_path = relative_path(&self.root_directory, &repository_path)?;
        let display_path = configured_display_path(
            &self.resolved,
            &repository_path,
            &display_path,
            name.as_str(),
        )?;
        let mut candidate = self.document.clone();
        insert_repository(
            &mut candidate,
            name.as_str(),
            &display_path,
            url.as_config_value(),
            &self.destination,
        )?;
        let candidate_contents = candidate.to_string();
        let resolved =
            load_with_replacement(&self.root_path, &self.destination, &candidate_contents)?;
        verify_added_repository(&resolved, &name, &repository_path, &url)?;

        let persistence = if self.dry_run {
            Persistence::Durable
        } else {
            persist(&self.destination, self.baseline.as_deref(), candidate_contents.as_bytes())?
        };
        if !self.dry_run {
            self.baseline = Some(candidate_contents);
        }
        self.document = candidate;
        self.resolved = resolved;

        match persistence {
            Persistence::Durable => {
                Ok(Addition::Added { name: name.as_str().to_string(), display_path })
            }
            Persistence::DurabilityUnconfirmed(message) => Ok(Addition::DurabilityUnconfirmed {
                name: name.as_str().to_string(),
                display_path,
                message,
            }),
        }
    }
}

fn configured_display_path(
    resolved: &ResolvedConfig,
    repository_path: &Path,
    default: &str,
    name: &str,
) -> Result<String, AppError> {
    let Some(parent) = repository_path.parent() else {
        return Ok(default.to_string());
    };
    let mut configured_parents = Vec::<&str>::new();

    for group in resolved.groups() {
        if group.path() != parent {
            continue;
        }
        let configured_parent = group.display_path();
        if !configured_parents.contains(&configured_parent) {
            configured_parents.push(configured_parent);
        }
    }

    match configured_parents.as_slice() {
        [] => Ok(default.to_string()),
        [configured_parent] => Ok(format!("{configured_parent}/{name}")),
        _ => Err(AppError::config_error(format!(
            "repository directory '{}' is represented by multiple configured paths: {}; consolidate them before adding '{name}'",
            parent.display(),
            configured_parents
                .iter()
                .map(|path| format!("'{path}'"))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn verify_added_repository(
    resolved: &ResolvedConfig,
    name: &RepositoryName,
    repository_path: &Path,
    url: &RemoteUrl,
) -> Result<(), AppError> {
    let Some(repository) = resolved.repositories().iter().find(|entry| entry.name() == name) else {
        return Err(AppError::config_error(format!(
            "prospective configuration does not contain added repository '{name}'; no changes were written"
        )));
    };
    if repository.path() != repository_path {
        return Err(AppError::config_error(format!(
            "prospective configuration resolves added repository '{name}' to '{}' instead of requested worktree '{}'; no changes were written",
            repository.path().display(),
            repository_path.display()
        )));
    }
    if !repository.url().matches(url) {
        return Err(AppError::config_error(format!(
            "prospective configuration changes the URL for added repository '{name}'; no changes were written"
        )));
    }
    Ok(())
}

struct DirectoryLock {
    file: File,
}

impl DirectoryLock {
    fn acquire(directory: &Path, shared: bool) -> Result<Self, AppError> {
        let file = File::open(directory)?;
        if shared {
            file.lock_shared()?;
        } else {
            file.lock()?;
        }
        Ok(Self { file })
    }
}

impl Drop for DirectoryLock {
    fn drop(&mut self) {
        let _ = File::unlock(&self.file);
    }
}

fn relative_path(root: &Path, repository: &Path) -> Result<String, AppError> {
    let relative = repository.strip_prefix(root).map_err(|_| {
        AppError::config_error(format!(
            "repository '{}' leaves the grove root '{}'; choose a broader grove.toml",
            repository.display(),
            root.display()
        ))
    })?;
    if relative.as_os_str().is_empty() {
        return Ok(".".to_string());
    }

    relative
        .components()
        .map(|component| match component {
            Component::Normal(value) => value.to_str().map(str::to_string).ok_or_else(|| {
                AppError::config_error(format!(
                    "repository path '{}' is not valid UTF-8 and cannot be written to TOML",
                    repository.display()
                ))
            }),
            _ => {
                Err(AppError::internal("canonical repository path contained an invalid component"))
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("/"))
}

fn insert_repository(
    document: &mut DocumentMut,
    name: &str,
    display_path: &str,
    url: &str,
    destination: &Path,
) -> Result<(), AppError> {
    if display_path == name {
        return insert_root_repository(document, name, None, url, destination);
    }
    if let Some((group, leaf)) = display_path.rsplit_once('/')
        && leaf == name
        && !group.is_empty()
    {
        return insert_group_repository(document, group, name, url, destination);
    }

    insert_root_repository(document, name, Some(display_path), url, destination)
}

fn insert_root_repository(
    document: &mut DocumentMut,
    name: &str,
    path: Option<&str>,
    url: &str,
    destination: &Path,
) -> Result<(), AppError> {
    if !document.as_table().contains_key("repos") {
        let mut repositories = Table::new();
        repositories.set_implicit(true);
        document.as_table_mut().insert("repos", Item::Table(repositories));
    }

    let repositories = document
        .as_table_mut()
        .get_mut("repos")
        .ok_or_else(|| AppError::internal("validated configuration lost its repos table"))?;
    match repositories {
        Item::Table(repositories) => {
            if path.is_none() && repositories.iter().all(|(_, item)| !item.is_table()) {
                repositories.insert(name, value(url));
                return Ok(());
            }
            let mut repository = Table::new();
            if let Some(path) = path {
                repository.insert("path", value(path));
            }
            repository.insert("url", value(url));
            repositories.insert(name, Item::Table(repository));
        }
        Item::Value(Value::InlineTable(repositories)) => {
            if path.is_none() {
                repositories.insert(name, Value::from(url));
                return Ok(());
            }
            let mut repository = InlineTable::new();
            if let Some(path) = path {
                repository.insert("path", Value::from(path));
            }
            repository.insert("url", Value::from(url));
            repositories.insert(name, Value::InlineTable(repository));
        }
        _ => {
            return Err(AppError::config_error(format!(
                "{}: field 'repos' must be a table",
                destination.display()
            )));
        }
    }
    Ok(())
}

fn insert_group_repository(
    document: &mut DocumentMut,
    group: &str,
    name: &str,
    url: &str,
    destination: &Path,
) -> Result<(), AppError> {
    if !document.as_table().contains_key("groups") {
        let mut groups = Table::new();
        groups.set_implicit(true);
        document.as_table_mut().insert("groups", Item::Table(groups));
    }

    let groups = document
        .as_table_mut()
        .get_mut("groups")
        .ok_or_else(|| AppError::internal("validated configuration lost its groups table"))?;
    match groups {
        Item::Table(groups) => {
            if !groups.contains_key(group) {
                groups.insert(group, Item::Table(Table::new()));
            }
            let repositories = groups.get_mut(group).ok_or_else(|| {
                AppError::internal("inserted configuration group could not be read")
            })?;
            match repositories {
                Item::Table(repositories) => {
                    if repositories.iter().all(|(_, item)| !item.is_table()) {
                        repositories.insert(name, value(url));
                    } else {
                        let mut repository = Table::new();
                        repository.insert("url", value(url));
                        repositories.insert(name, Item::Table(repository));
                    }
                }
                Item::Value(Value::InlineTable(repositories)) => {
                    repositories.insert(name, Value::from(url));
                }
                _ => {
                    return Err(AppError::config_error(format!(
                        "{}: group '{group}' must be a table",
                        destination.display()
                    )));
                }
            }
        }
        Item::Value(Value::InlineTable(groups)) => {
            if !groups.contains_key(group) {
                groups.insert(group, Value::InlineTable(InlineTable::new()));
            }
            let repositories = groups.get_mut(group).ok_or_else(|| {
                AppError::internal("inserted inline configuration group could not be read")
            })?;
            let Value::InlineTable(repositories) = repositories else {
                return Err(AppError::config_error(format!(
                    "{}: group '{group}' must be a table",
                    destination.display()
                )));
            };
            repositories.insert(name, Value::from(url));
        }
        _ => {
            return Err(AppError::config_error(format!(
                "{}: field 'groups' must be a table",
                destination.display()
            )));
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum Persistence {
    Durable,
    DurabilityUnconfirmed(String),
}

fn persist(path: &Path, baseline: Option<&str>, contents: &[u8]) -> Result<Persistence, AppError> {
    persist_with_directory_sync(path, baseline, contents, File::sync_all)
}

fn persist_with_directory_sync(
    path: &Path,
    baseline: Option<&str>,
    contents: &[u8],
    sync_directory: impl FnOnce(&File) -> io::Result<()>,
) -> Result<Persistence, AppError> {
    match (baseline, fs::read_to_string(path)) {
        (Some(expected), Ok(actual)) if actual == expected => {}
        (Some(_), Ok(_)) => {
            return Err(AppError::config_error(format!(
                "{} changed while gv add was running; no changes were written",
                path.display()
            )));
        }
        (Some(_), Err(error)) => return Err(error.into()),
        (None, Err(error)) if error.kind() == io::ErrorKind::NotFound => {}
        (None, _) => {
            return Err(AppError::config_error(format!(
                "{} appeared while gv add was running; no changes were written",
                path.display()
            )));
        }
    }

    let directory = path
        .parent()
        .ok_or_else(|| AppError::config_error(format!("{} has no parent", path.display())))?;
    let directory_file = File::open(directory)?;
    let mut temporary = NamedTempFile::new_in(directory)?;
    if path.exists() {
        temporary.as_file().set_permissions(fs::metadata(path)?.permissions())?;
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temporary.as_file().set_permissions(fs::Permissions::from_mode(0o600))?;
        }
    }
    temporary.write_all(contents)?;
    temporary.as_file_mut().sync_all()?;
    temporary.persist(path).map_err(|error| AppError::from(error.error))?;
    match sync_directory(&directory_file) {
        Ok(()) => Ok(Persistence::Durable),
        Err(error) => Ok(Persistence::DurabilityUnconfirmed(format!(
            "{} was written, but syncing its directory failed: {error}; verify the file before retrying",
            path.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;

    use tempfile::TempDir;

    use super::{Addition, EditSession, Persistence, persist, persist_with_directory_sync};
    use crate::repositories::{RemoteUrl, RepositoryName};

    #[test]
    fn adds_a_repository_without_reformatting_existing_content() {
        let root = TempDir::new().unwrap();
        let config = root.path().join("grove.toml");
        let repository = root.path().join("backend");
        fs::create_dir(&repository).unwrap();
        fs::write(
            &config,
            "# retained\nversion = 2\n\n[repos.frontend]\nurl = 'git@example.com:frontend.git' # retained\n",
        )
        .unwrap();
        let mut session = EditSession::open(&config, false, false).unwrap();

        let outcome = session
            .add(
                RepositoryName::new("backend").unwrap(),
                &repository,
                RemoteUrl::new("git@example.com:backend.git").unwrap(),
            )
            .unwrap();

        assert_eq!(
            outcome,
            Addition::Added { name: "backend".to_string(), display_path: "backend".to_string() }
        );
        let contents = fs::read_to_string(config).unwrap();
        assert!(contents.starts_with(
            "# retained\nversion = 2\n\n[repos.frontend]\nurl = 'git@example.com:frontend.git' # retained\n"
        ));
        assert!(contents.contains("[repos.backend]\nurl = \"git@example.com:backend.git\""));
    }

    #[test]
    fn dry_run_accumulates_candidates_without_writing() {
        let root = TempDir::new().unwrap();
        let config = root.path().join("grove.toml");
        fs::write(&config, "version = 2\n").unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let mut session = EditSession::open(&config, false, true).unwrap();

        session
            .add(
                RepositoryName::new("first").unwrap(),
                &first,
                RemoteUrl::new("git@example.com:first.git").unwrap(),
            )
            .unwrap();
        session
            .add(
                RepositoryName::new("second").unwrap(),
                &second,
                RemoteUrl::new("git@example.com:second.git").unwrap(),
            )
            .unwrap();

        assert_eq!(fs::read_to_string(config).unwrap(), "version = 2\n");
    }

    #[test]
    fn creates_a_versionless_override_only_after_a_successful_addition() {
        let root = TempDir::new().unwrap();
        let config = root.path().join("grove.toml");
        fs::write(&config, "version = 2\n").unwrap();
        let repository = root.path().join("repo");
        fs::create_dir(&repository).unwrap();
        let mut session = EditSession::open(&config, true, false).unwrap();
        let override_path = root.path().join("grove.override.toml");
        assert!(!override_path.exists());

        session
            .add(
                RepositoryName::new("repo").unwrap(),
                &repository,
                RemoteUrl::new("git@example.com:repo.git").unwrap(),
            )
            .unwrap();

        let contents = fs::read_to_string(override_path).unwrap();
        assert!(!contents.contains("version"));
        assert!(contents.contains("[repos]\nrepo = \"git@example.com:repo.git\""));
    }

    #[test]
    fn preserves_an_inline_repositories_table() {
        let root = TempDir::new().unwrap();
        let config = root.path().join("grove.toml");
        let repository = root.path().join("backend");
        fs::create_dir(&repository).unwrap();
        fs::write(
            &config,
            "version = 2\nrepos = { frontend = { url = 'git@example.com:frontend.git' } } # retained\n",
        )
        .unwrap();
        let mut session = EditSession::open(&config, false, false).unwrap();

        session
            .add(
                RepositoryName::new("backend").unwrap(),
                &repository,
                RemoteUrl::new("git@example.com:backend.git").unwrap(),
            )
            .unwrap();

        let contents = fs::read_to_string(config).unwrap();
        assert!(contents.contains("frontend = { url = 'git@example.com:frontend.git' }"));
        assert!(contents.contains("backend = \"git@example.com:backend.git\""));
        assert!(contents.ends_with("# retained\n"));
    }

    #[test]
    fn preserves_dotted_repository_keys() {
        let root = TempDir::new().unwrap();
        let config = root.path().join("grove.toml");
        let repository = root.path().join("backend");
        fs::create_dir(&repository).unwrap();
        fs::write(
            &config,
            "version = 2\nrepos.frontend.url = 'git@example.com:frontend.git' # retained\n",
        )
        .unwrap();
        let mut session = EditSession::open(&config, false, false).unwrap();

        session
            .add(
                RepositoryName::new("backend").unwrap(),
                &repository,
                RemoteUrl::new("git@example.com:backend.git").unwrap(),
            )
            .unwrap();

        let contents = fs::read_to_string(config).unwrap();
        assert!(
            contents.contains("repos.frontend.url = 'git@example.com:frontend.git' # retained")
        );
        assert!(contents.contains("[repos.backend]"));
    }

    #[test]
    fn writes_dot_for_a_repository_at_the_grove_root() {
        let root = TempDir::new().unwrap();
        let repository = root.path().join("self-managed");
        fs::create_dir(&repository).unwrap();
        let config = repository.join("grove.toml");
        fs::write(&config, "version = 2\n").unwrap();
        let mut session = EditSession::open(&config, false, false).unwrap();

        session
            .add(
                RepositoryName::new("self-managed").unwrap(),
                &repository,
                RemoteUrl::new("git@example.com:self-managed.git").unwrap(),
            )
            .unwrap();

        assert!(fs::read_to_string(config).unwrap().contains("path = \".\""));
    }

    #[test]
    fn quotes_a_repository_name_containing_a_dot() {
        let root = TempDir::new().unwrap();
        let config = root.path().join("grove.toml");
        let repository = root.path().join("company.service");
        fs::create_dir(&repository).unwrap();
        fs::write(&config, "version = 2\n").unwrap();
        let mut session = EditSession::open(&config, false, false).unwrap();

        session
            .add(
                RepositoryName::new("company.service").unwrap(),
                &repository,
                RemoteUrl::new("git@example.com:company/service.git").unwrap(),
            )
            .unwrap();

        assert!(
            fs::read_to_string(config)
                .unwrap()
                .contains("\"company.service\" = \"git@example.com:company/service.git\"")
        );
    }

    #[test]
    fn groups_a_repository_by_its_parent_directory() {
        let root = TempDir::new().unwrap();
        let config = root.path().join("grove.toml");
        let repository = root.path().join("Clients").join("acme").join("backend");
        fs::create_dir_all(&repository).unwrap();
        fs::write(&config, "version = 2\n").unwrap();
        let mut session = EditSession::open(&config, false, false).unwrap();

        session
            .add(
                RepositoryName::new("backend").unwrap(),
                &repository,
                RemoteUrl::new("git@example.com:backend.git").unwrap(),
            )
            .unwrap();

        assert!(
            fs::read_to_string(config)
                .unwrap()
                .contains("[groups.\"Clients/acme\"]\nbackend = \"git@example.com:backend.git\"")
        );
    }

    #[test]
    fn detects_a_changed_destination_before_replacement() {
        let root = TempDir::new().unwrap();
        let path = root.path().join("grove.toml");
        fs::write(&path, "changed\n").unwrap();

        let error = persist(&path, Some("original\n"), b"replacement\n").unwrap_err();

        assert!(error.to_string().contains("changed while gv add was running"));
        assert_eq!(fs::read_to_string(path).unwrap(), "changed\n");
    }

    #[test]
    fn reports_unconfirmed_durability_after_replacing_the_destination() {
        let root = TempDir::new().unwrap();
        let path = root.path().join("grove.toml");
        fs::write(&path, "original\n").unwrap();

        let outcome =
            persist_with_directory_sync(&path, Some("original\n"), b"replacement\n", |_| {
                Err(io::Error::other("sync failed"))
            })
            .unwrap();

        assert!(
            matches!(outcome, Persistence::DurabilityUnconfirmed(message) if message.contains("was written"))
        );
        assert_eq!(fs::read_to_string(path).unwrap(), "replacement\n");
    }

    #[cfg(unix)]
    #[test]
    fn preserves_existing_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().unwrap();
        let config = root.path().join("grove.toml");
        let repository = root.path().join("repo");
        fs::create_dir(&repository).unwrap();
        fs::write(&config, "version = 2\n").unwrap();
        fs::set_permissions(&config, fs::Permissions::from_mode(0o640)).unwrap();
        let mut session = EditSession::open(&config, false, false).unwrap();

        session
            .add(
                RepositoryName::new("repo").unwrap(),
                &repository,
                RemoteUrl::new("git@example.com:repo.git").unwrap(),
            )
            .unwrap();

        assert_eq!(fs::metadata(config).unwrap().permissions().mode() & 0o777, 0o640);
    }

    #[cfg(unix)]
    #[test]
    fn creates_an_owner_only_override() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().unwrap();
        let config = root.path().join("grove.toml");
        let repository = root.path().join("repo");
        fs::create_dir(&repository).unwrap();
        fs::write(&config, "version = 2\n").unwrap();
        let mut session = EditSession::open(&config, true, false).unwrap();

        session
            .add(
                RepositoryName::new("repo").unwrap(),
                &repository,
                RemoteUrl::new("git@example.com:repo.git").unwrap(),
            )
            .unwrap();

        let override_path = root.path().join("grove.override.toml");
        assert_eq!(fs::metadata(override_path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_an_existing_override_symlink() {
        use std::os::unix::fs::symlink;

        let root = TempDir::new().unwrap();
        let config = root.path().join("grove.toml");
        let override_target = root.path().join("local.toml");
        let override_link = root.path().join("grove.override.toml");
        let repository = root.path().join("repo");
        fs::create_dir(&repository).unwrap();
        fs::write(&config, "version = 2\n").unwrap();
        fs::write(&override_target, "").unwrap();
        symlink(&override_target, &override_link).unwrap();
        let mut session = EditSession::open(&config, true, false).unwrap();

        session
            .add(
                RepositoryName::new("repo").unwrap(),
                &repository,
                RemoteUrl::new("git@example.com:repo.git").unwrap(),
            )
            .unwrap();

        assert!(fs::symlink_metadata(override_link).unwrap().file_type().is_symlink());
        assert!(
            fs::read_to_string(override_target)
                .unwrap()
                .contains("[repos]\nrepo = \"git@example.com:repo.git\"")
        );
    }

    #[test]
    fn edit_session_locks_the_selected_base_directory() {
        let root = TempDir::new().unwrap();
        let config = root.path().join("grove.toml");
        fs::write(&config, "version = 2\n").unwrap();
        let session = EditSession::open(&config, false, false).unwrap();
        let competing = fs::File::open(root.path()).unwrap();

        assert!(matches!(competing.try_lock(), Err(fs::TryLockError::WouldBlock)));

        drop(session);
        competing.try_lock().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn edit_session_locks_the_override_target_directory() {
        use std::os::unix::fs::symlink;

        let root = TempDir::new().unwrap();
        let target_directory = TempDir::new().unwrap();
        let config = root.path().join("grove.toml");
        let override_target = target_directory.path().join("local.toml");
        fs::write(&config, "version = 2\n").unwrap();
        fs::write(&override_target, "").unwrap();
        symlink(&override_target, root.path().join("grove.override.toml")).unwrap();

        let session = EditSession::open(&config, true, false).unwrap();
        let competing_root = fs::File::open(root.path()).unwrap();
        let competing_target = fs::File::open(target_directory.path()).unwrap();

        assert!(matches!(competing_root.try_lock(), Err(fs::TryLockError::WouldBlock)));
        assert!(matches!(competing_target.try_lock(), Err(fs::TryLockError::WouldBlock)));

        drop(session);
        competing_root.try_lock().unwrap();
        competing_target.try_lock().unwrap();
    }
}
