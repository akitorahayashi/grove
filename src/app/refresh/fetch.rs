use crate::git::GitClient;
use crate::phases::{EventProgress, EventSink, Task as PhaseTask};

use super::task::Task;
use super::{BlockedReason, Entry, Outcome, Phase};

pub(super) enum Completion<'a> {
    Entry { index: usize, entry: Entry },
    Refresh(Task<'a>),
}

impl Completion<'_> {
    pub(super) fn fetched(&self) -> bool {
        matches!(self, Self::Refresh(_))
    }
}

pub(super) fn repository<'a>(
    git: &impl GitClient,
    task: &Task<'a>,
    events: &impl EventSink<Phase>,
) -> Result<Completion<'a>, crate::AppError> {
    let mut progress = EventProgress::new(task.repository(), events);

    Ok(match git.fetch(task.repository().path(), &mut progress) {
        Ok(()) => Completion::Refresh(task.clone()),
        Err(error) if error.is_internal() => return Err(error),
        Err(error) => Completion::Entry {
            index: task.index(),
            entry: Entry::new(
                task.repository(),
                Outcome::Blocked { reason: BlockedReason::FetchFailed(error.to_string()) },
            ),
        },
    })
}
