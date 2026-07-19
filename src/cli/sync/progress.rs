use std::collections::HashMap;
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

use crate::app::sync::{Event, Phase, PhaseSummary};
use crate::git::GitProgress;

use super::super::printer::Printer;

const MINIMUM_NAME_WIDTH: usize = 20;

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
    multi: MultiProgress,
    root: Option<Root>,
    children: HashMap<String, Child>,
    name_width: usize,
}

#[derive(Debug)]
struct Root {
    phase: Phase,
    progress: ProgressBar,
}

#[derive(Debug)]
struct Child {
    progress: ProgressBar,
    numeric: bool,
}

impl Display {
    pub(super) fn new(printer: Printer) -> Self {
        Self::with_target(printer.target())
    }

    fn with_target(target: ProgressDrawTarget) -> Self {
        Self {
            multi: MultiProgress::with_draw_target(target),
            root: None,
            children: HashMap::new(),
            name_width: MINIMUM_NAME_WIDTH,
        }
    }

    pub(super) fn handle(&mut self, event: Event) -> Option<Completion> {
        match event {
            Event::PhaseStarted { phase, total } => {
                self.start_phase(phase, total);
                None
            }
            Event::RepositoryStarted { repository, phase } => {
                self.start_repository(repository, phase);
                None
            }
            Event::GitProgress { repository, progress } => {
                self.update_repository(&repository, &progress);
                None
            }
            Event::RepositoryFinished { repository, phase } => {
                self.finish_repository(&repository, phase);
                None
            }
            Event::PhaseCompleted { phase, summary } => {
                self.finish_phase(phase);
                Some(Completion { phase, summary })
            }
            Event::PhaseFailed { phase } => {
                self.finish_phase(phase);
                None
            }
        }
    }

    pub(super) fn finish(&mut self) {
        self.clear_progress();
    }

    fn start_phase(&mut self, phase: Phase, total: usize) {
        self.clear_progress();
        let progress = self.multi.add(ProgressBar::new(total as u64));
        progress.enable_steady_tick(Duration::from_millis(200));
        progress.set_style(root_style());
        progress.set_message(phase.message());
        progress.tick();
        self.root = Some(Root { phase, progress });
    }

    fn start_repository(&mut self, repository: String, phase: Phase) {
        if phase != Phase::Preparing || !self.phase_is_active(phase) {
            return;
        }

        self.name_width = self.name_width.max(repository.chars().count());
        self.refresh_child_styles();

        let progress = self.multi.add(ProgressBar::new_spinner());
        progress.set_style(unknown_style(self.name_width));
        progress.set_message(repository.clone());
        progress.tick();
        self.children.insert(repository, Child { progress, numeric: false });
    }

    fn update_repository(&mut self, repository: &str, progress: &GitProgress) {
        let Some(child) = self.children.get_mut(repository) else {
            return;
        };

        let bounds = match (progress.current(), progress.total(), progress.percent()) {
            (Some(current), Some(total), _) if total > 0 => Some((current, total)),
            (_, _, Some(percent)) => Some((u64::from(percent), 100)),
            _ => None,
        };
        let Some((position, length)) = bounds else {
            return;
        };

        child.numeric = true;
        child.progress.set_style(numeric_style(self.name_width));
        child.progress.set_length(length);
        child.progress.set_position(position.min(length));
        child.progress.tick();
    }

    fn finish_repository(&mut self, repository: &str, phase: Phase) {
        if !self.phase_is_active(phase) {
            return;
        }

        if phase == Phase::Preparing
            && let Some(child) = self.children.remove(repository)
        {
            child.progress.finish_and_clear();
        }
        if let Some(root) = &self.root {
            root.progress.inc(1);
        }
    }

    fn finish_phase(&mut self, phase: Phase) {
        if self.phase_is_active(phase) {
            self.clear_progress();
        }
    }

    fn phase_is_active(&self, phase: Phase) -> bool {
        self.root.as_ref().is_some_and(|root| root.phase == phase)
    }

    fn refresh_child_styles(&self) {
        for child in self.children.values() {
            let style = if child.numeric {
                numeric_style(self.name_width)
            } else {
                unknown_style(self.name_width)
            };
            child.progress.set_style(style);
            child.progress.tick();
        }
    }

    fn clear_progress(&mut self) {
        for (_, child) in self.children.drain() {
            child.progress.finish_and_clear();
        }
        if let Some(root) = self.root.take() {
            root.progress.set_message("");
            root.progress.finish_and_clear();
        }
        self.name_width = MINIMUM_NAME_WIDTH;
    }
}

fn root_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner:.white} {msg:.dim} ({pos}/{len})")
        .expect("sync root progress template should be valid")
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
}

fn unknown_style(name_width: usize) -> ProgressStyle {
    ProgressStyle::with_template(&format!("{{msg:{name_width}.dim}} ...."))
        .expect("sync pending progress template should be valid")
}

fn numeric_style(name_width: usize) -> ProgressStyle {
    ProgressStyle::with_template(&format!(
        "{{msg:{name_width}.dim}} {{bar:30.green/black.dim}} {{pos:>7}}/{{len:7}}"
    ))
    .expect("sync numeric progress template should be valid")
    .progress_chars("--")
}

#[cfg(test)]
mod tests {
    use indicatif::{InMemoryTerm, ProgressDrawTarget};

    use super::Display;
    use crate::app::sync::{Event, Phase, PhaseSummary};
    use crate::git::GitProgress;

    #[test]
    fn renders_root_and_repository_progress_then_clears() {
        let terminal = InMemoryTerm::new(10, 100);
        let target = ProgressDrawTarget::term_like(Box::new(terminal.clone()));
        let mut display = Display::with_target(target);

        display.handle(Event::PhaseStarted { phase: Phase::Preparing, total: 2 });
        display.handle(Event::RepositoryStarted {
            repository: "blog".to_string(),
            phase: Phase::Preparing,
        });
        display.handle(Event::GitProgress {
            repository: "blog".to_string(),
            progress: GitProgress::new("Receiving objects", Some(42), Some(128), Some(302)),
        });

        let contents = terminal.contents();
        assert!(contents.contains("Preparing repositories... (0/2)"));
        assert!(contents.contains("blog"));
        assert!(contents.contains("128"));
        assert!(contents.contains("302"));

        display.handle(Event::RepositoryFinished {
            repository: "blog".to_string(),
            phase: Phase::Preparing,
        });
        display.handle(Event::PhaseCompleted {
            phase: Phase::Preparing,
            summary: PhaseSummary::default(),
        });

        assert!(terminal.contents().is_empty());
    }
}
