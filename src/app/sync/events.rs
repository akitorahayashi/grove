use std::sync::mpsc::Sender;

use crate::git::GitProgress;

use super::PhaseSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Checking,
    Preparing,
    Updating,
}

impl Phase {
    pub fn message(self) -> &'static str {
        match self {
            Self::Checking => "Checking repositories...",
            Self::Preparing => "Preparing repositories...",
            Self::Updating => "Updating repositories...",
        }
    }
}

#[derive(Debug)]
pub(crate) enum Event {
    PhaseStarted { phase: Phase, total: usize },
    RepositoryStarted { repository: String, phase: Phase },
    GitProgress { repository: String, progress: GitProgress },
    RepositoryFinished { repository: String, phase: Phase },
    PhaseCompleted { phase: Phase, summary: PhaseSummary },
    PhaseFailed { phase: Phase },
}

pub(crate) trait EventSink: Sync {
    fn emit(&self, event: Event);
}

impl EventSink for Sender<Event> {
    fn emit(&self, event: Event) {
        self.send(event).expect("sync event receiver should remain connected");
    }
}

#[derive(Debug, Default)]
pub(super) struct DiscardEvents;

impl EventSink for DiscardEvents {
    fn emit(&self, _event: Event) {}
}
