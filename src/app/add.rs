use std::fs;
use std::path::{Path, PathBuf};

use crate::AppError;
use crate::app::AppContext;
use crate::config::{Addition, EditSession};
use crate::git::RepositoryProbe;
use crate::repositories::RepositoryName;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Options {
    override_file: bool,
    dry_run: bool,
}

impl Options {
    pub(crate) fn new(override_file: bool, dry_run: bool) -> Self {
        Self { override_file, dry_run }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    input: PathBuf,
    name: Option<String>,
    display_path: Option<String>,
    outcome: Outcome,
}

impl Entry {
    pub(crate) fn input(&self) -> &Path {
        &self.input
    }

    pub(crate) fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub(crate) fn display_path(&self) -> Option<&str> {
        self.display_path.as_deref()
    }

    pub(crate) fn outcome(&self) -> &Outcome {
        &self.outcome
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    Added,
    Planned,
    Unchanged,
    Failed(String),
}

pub(crate) enum Event<'a> {
    Destination(&'a Path),
    Entry(&'a Entry),
}

#[derive(Debug, Clone)]
pub(crate) struct Report {
    entries: Vec<Entry>,
    total: usize,
    dry_run: bool,
}

impl Report {
    pub(crate) fn added(&self) -> usize {
        self.entries.iter().filter(|entry| entry.outcome == Outcome::Added).count()
    }

    pub(crate) fn planned(&self) -> usize {
        self.entries.iter().filter(|entry| entry.outcome == Outcome::Planned).count()
    }

    pub(crate) fn unchanged(&self) -> usize {
        self.entries.iter().filter(|entry| entry.outcome == Outcome::Unchanged).count()
    }

    pub(crate) fn failed(&self) -> usize {
        self.entries.iter().filter(|entry| matches!(entry.outcome, Outcome::Failed(_))).count()
    }

    pub(crate) fn not_attempted(&self) -> usize {
        self.total.saturating_sub(self.entries.len())
    }

    pub(crate) fn has_failure(&self) -> bool {
        self.failed() > 0
    }

    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }
}

pub(crate) fn execute<G, Z, F>(
    ctx: &AppContext<G, Z>,
    config_path: &Path,
    cwd: &Path,
    paths: &[PathBuf],
    options: Options,
    mut on_event: F,
) -> Result<Report, AppError>
where
    G: RepositoryProbe,
    F: FnMut(Event<'_>) -> Result<(), AppError>,
{
    ctx.git().verify_available()?;
    let default_path = PathBuf::from(".");
    let paths = if paths.is_empty() { std::slice::from_ref(&default_path) } else { paths };
    let mut editor = EditSession::open(config_path, options.override_file, options.dry_run)?;
    on_event(Event::Destination(editor.destination()))?;
    let mut entries = Vec::with_capacity(paths.len());

    for input in paths {
        let entry = match add_one(ctx.git(), &mut editor, cwd, input, options.dry_run) {
            Ok(entry) => entry,
            Err(error) => Entry {
                input: input.clone(),
                name: None,
                display_path: None,
                outcome: Outcome::Failed(error.demote()?),
            },
        };
        let failed = matches!(entry.outcome, Outcome::Failed(_));
        entries.push(entry);
        on_event(Event::Entry(entries.last().expect("entry was just appended")))?;
        if failed {
            break;
        }
    }

    Ok(Report { entries, total: paths.len(), dry_run: options.dry_run })
}

fn add_one(
    git: &impl RepositoryProbe,
    editor: &mut EditSession,
    cwd: &Path,
    input: &Path,
    dry_run: bool,
) -> Result<Entry, AppError> {
    let candidate = if input.is_absolute() { input.to_path_buf() } else { cwd.join(input) };
    let metadata = fs::metadata(&candidate).map_err(|error| {
        AppError::invalid_arguments(format!(
            "repository path '{}' is unavailable: {error}; provide an existing directory",
            input.display()
        ))
    })?;
    if !metadata.is_dir() {
        return Err(AppError::invalid_arguments(format!(
            "repository path '{}' is not a directory; provide a directory inside a Git worktree",
            input.display()
        )));
    }
    let candidate = candidate.canonicalize()?;
    let Some(repository_path) = git.worktree_root(&candidate)? else {
        return Err(AppError::invalid_arguments(format!(
            "repository path '{}' is not inside a Git worktree; initialize or clone the repository first",
            input.display()
        )));
    };
    let name = repository_path.file_name().and_then(|value| value.to_str()).ok_or_else(|| {
        AppError::invalid_arguments(format!(
            "cannot derive a UTF-8 repository name from '{}'; rename the directory or add the entry manually",
            repository_path.display()
        ))
    })?;
    let name = RepositoryName::new(name).map_err(|_| {
        AppError::invalid_arguments(format!(
            "cannot derive a repository name from '{}'; names may contain only ASCII letters, digits, '-', '_' and '.'; rename the directory or add the entry manually",
            repository_path.display()
        ))
    })?;
    let url = git.remote_url(&repository_path)?.ok_or_else(|| {
        AppError::invalid_arguments(format!(
            "repository '{}' has no remote origin; configure a credential-free origin URL first",
            repository_path.display()
        ))
    })?;
    url.ensure_safe_for_config()?;

    match editor.add(name, &repository_path, url)? {
        Addition::Added { name, display_path } => Ok(Entry {
            input: input.to_path_buf(),
            name: Some(name),
            display_path: Some(display_path),
            outcome: if dry_run { Outcome::Planned } else { Outcome::Added },
        }),
        Addition::Unchanged { name, display_path } => Ok(Entry {
            input: input.to_path_buf(),
            name: Some(name),
            display_path: Some(display_path),
            outcome: Outcome::Unchanged,
        }),
    }
}
