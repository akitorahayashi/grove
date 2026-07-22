use std::collections::HashMap;
use std::fmt::Debug;
use std::io;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

use crate::AppError;
use crate::git::GitProgress;
use crate::phases::{Event, Summary};

use crate::cli::output::{Output, terminal_text};

const MINIMUM_NAME_WIDTH: usize = 20;

pub(in crate::cli) trait ProgressPhase: Copy + Debug + Eq {
    fn message(self) -> &'static str;
    fn shows_git_progress(self) -> bool;
}

#[derive(Debug)]
pub(in crate::cli) struct RepositoryProgress<P: ProgressPhase> {
    multi: MultiProgress,
    root: Option<Root<P>>,
    children: HashMap<String, Child>,
    name_width: usize,
}

#[derive(Debug)]
struct Root<P> {
    phase: P,
    progress: ProgressBar,
}

#[derive(Debug)]
struct Child {
    progress: ProgressBar,
    numeric: bool,
}

impl<P: ProgressPhase> RepositoryProgress<P> {
    pub(in crate::cli) fn new() -> Self {
        Self::with_target(ProgressDrawTarget::stderr())
    }

    fn with_target(target: ProgressDrawTarget) -> Self {
        Self {
            multi: MultiProgress::with_draw_target(target),
            root: None,
            children: HashMap::new(),
            name_width: MINIMUM_NAME_WIDTH,
        }
    }

    pub(in crate::cli) fn start_phase(&mut self, phase: P, total: usize) {
        self.clear_progress();
        let progress = self.multi.add(ProgressBar::new(total as u64));
        progress.enable_steady_tick(Duration::from_millis(200));
        progress.set_style(root_style());
        progress.set_message(phase.message());
        progress.tick();
        self.root = Some(Root { phase, progress });
    }

    pub(in crate::cli) fn start_repository(&mut self, repository: String, phase: P) {
        if !phase.shows_git_progress() || !self.phase_is_active(phase) {
            return;
        }

        let displayed = terminal_text(&repository);
        self.name_width = self.name_width.max(displayed.chars().count());
        self.refresh_child_styles();

        let progress = self.multi.add(ProgressBar::new_spinner());
        progress.set_style(unknown_style(self.name_width));
        progress.set_message(displayed);
        progress.tick();
        self.children.insert(repository, Child { progress, numeric: false });
    }

    pub(in crate::cli) fn update_repository(&mut self, repository: &str, progress: &GitProgress) {
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

    pub(in crate::cli) fn finish_repository(&mut self, repository: &str, phase: P) {
        if !self.phase_is_active(phase) {
            return;
        }

        if phase.shows_git_progress()
            && let Some(child) = self.children.remove(repository)
        {
            child.progress.finish_and_clear();
        }
        if let Some(root) = &self.root {
            root.progress.inc(1);
        }
    }

    pub(in crate::cli) fn finish_phase(&mut self, phase: P) {
        if self.phase_is_active(phase) {
            self.clear_progress();
        }
    }

    pub(in crate::cli) fn finish(&mut self) {
        self.clear_progress();
    }

    fn phase_is_active(&self, phase: P) -> bool {
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
        .expect("repository root progress template should be valid")
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
}

fn unknown_style(name_width: usize) -> ProgressStyle {
    ProgressStyle::with_template(&format!("{{msg:{name_width}.dim}} ...."))
        .expect("repository pending progress template should be valid")
}

fn numeric_style(name_width: usize) -> ProgressStyle {
    ProgressStyle::with_template(&format!(
        "{{msg:{name_width}.dim}} {{bar:30.green/black.dim}} {{pos:>7}}/{{len:7}}"
    ))
    .expect("repository numeric progress template should be valid")
    .progress_chars("--")
}

struct Completion<P> {
    phase: P,
    summary: Summary,
}

struct Display<P: ProgressPhase> {
    progress: RepositoryProgress<P>,
}

impl<P: ProgressPhase> Display<P> {
    fn new() -> Self {
        Self { progress: RepositoryProgress::new() }
    }

    fn handle(&mut self, event: Event<P>) -> Option<Completion<P>> {
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

    fn finish(&mut self) {
        self.progress.finish();
    }
}

/// Drive a phase-emitting use case on a worker thread while rendering its
/// progress on this thread, printing each phase completion as it arrives.
pub(in crate::cli) fn run_with_progress<P, R>(
    output: &mut Output<'_>,
    thread_label: &str,
    execute: impl FnOnce(Sender<Event<P>>) -> Result<R, AppError> + Send,
    print_completion: impl Fn(P, Summary, &mut Output<'_>) -> io::Result<()>,
) -> Result<R, AppError>
where
    P: ProgressPhase + Send,
    R: Send,
{
    let (sender, receiver) = mpsc::channel();

    std::thread::scope(|scope| {
        let execution = scope.spawn(move || execute(sender));
        let mut progress = Display::new();
        let mut output_error = None;

        for event in receiver {
            if let Some(completion) = progress.handle(event)
                && output_error.is_none()
                && let Err(error) = print_completion(completion.phase, completion.summary, output)
            {
                output_error = Some(error);
            }
        }

        progress.finish();
        let report = execution.join().map_err(|_| {
            AppError::internal(format!("{thread_label} execution thread panicked"))
        })??;
        if let Some(error) = output_error {
            return Err(error.into());
        }
        Ok(report)
    })
}

#[cfg(test)]
mod tests {
    use indicatif::{InMemoryTerm, ProgressDrawTarget};

    use super::{ProgressPhase, RepositoryProgress};
    use crate::git::GitProgress;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TestPhase {
        Fetching,
    }

    impl ProgressPhase for TestPhase {
        fn message(self) -> &'static str {
            "Fetching repositories..."
        }

        fn shows_git_progress(self) -> bool {
            self == Self::Fetching
        }
    }

    #[test]
    fn renders_root_and_repository_progress_then_clears() {
        let terminal = InMemoryTerm::new(10, 100);
        let target = ProgressDrawTarget::term_like(Box::new(terminal.clone()));
        let mut display = RepositoryProgress::with_target(target);

        display.start_phase(TestPhase::Fetching, 2);
        display.start_repository("blog".to_string(), TestPhase::Fetching);
        display.update_repository("blog", &GitProgress::new(Some(42), Some(128), Some(302)));

        let contents = terminal.contents();
        assert!(contents.contains("Fetching repositories... (0/2)"));
        assert!(contents.contains("blog"));
        assert!(contents.contains("128"));
        assert!(contents.contains("302"));

        display.finish_repository("blog", TestPhase::Fetching);
        display.finish_phase(TestPhase::Fetching);

        assert!(terminal.contents().is_empty());
    }
}
