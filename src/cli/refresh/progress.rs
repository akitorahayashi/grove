use crate::app::refresh::{Event, Phase, PhaseSummary};
use crate::cli::repository_progress::{ProgressPhase, RepositoryProgress};

#[derive(Debug, Clone, Copy)]
pub(super) struct Completion {
    phase: Phase,
    summary: PhaseSummary,
}

impl Completion {
    pub(super) fn phase(self) -> Phase {
        self.phase
    }

    pub(super) fn summary(self) -> PhaseSummary {
        self.summary
    }
}

#[derive(Debug)]
pub(super) struct Display {
    progress: RepositoryProgress<Phase>,
}

impl Display {
    pub(super) fn new() -> Self {
        Self { progress: RepositoryProgress::new() }
    }

    pub(super) fn handle(&mut self, event: Event) -> Option<Completion> {
        match event {
            Event::PhaseStarted { phase, total } => self.progress.start_phase(phase, total),
            Event::RepositoryStarted { repository, phase } => {
                self.progress.start_repository(repository, phase);
            }
            Event::GitProgress { repository, progress } => {
                self.progress.update_repository(&repository, &progress);
            }
            Event::RepositoryFinished { repository, phase } => {
                self.progress.finish_repository(&repository, phase);
            }
            Event::PhaseCompleted { phase, summary } => {
                self.progress.finish_phase(phase);
                return Some(Completion { phase, summary });
            }
            Event::PhaseFailed { phase } => self.progress.finish_phase(phase),
        }
        None
    }

    pub(super) fn finish(&mut self) {
        self.progress.finish();
    }
}

impl ProgressPhase for Phase {
    fn message(self) -> &'static str {
        Phase::message(self)
    }

    fn shows_git_progress(self) -> bool {
        self == Phase::Fetching
    }
}
