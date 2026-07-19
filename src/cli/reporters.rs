use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use crate::app::sync::{Phase, SyncObserver};
use crate::git::GitProgress;

use super::printer::Printer;

#[derive(Debug)]
pub(super) struct SyncReporter {
    progress: ProgressBar,
    _multi: MultiProgress,
}

impl SyncReporter {
    pub(super) fn new(printer: Printer) -> Self {
        let multi = MultiProgress::with_draw_target(printer.target());
        let progress = multi.add(ProgressBar::with_draw_target(None, printer.target()));
        progress.enable_steady_tick(Duration::from_millis(200));
        progress.set_style(
            ProgressStyle::with_template("{spinner:.white} {wide_msg:.dim}")
                .expect("sync progress template should be valid"),
        );
        Self { progress, _multi: multi }
    }

    pub(super) fn finish(&self) {
        self.progress.set_message("");
        self.progress.finish_and_clear();
    }
}

impl SyncObserver for SyncReporter {
    fn phase_started(&mut self, repository: &str, phase: Phase) {
        self.progress.set_message(format!("{} {repository}", phase.message()));
    }

    fn git_progress(&mut self, repository: &str, phase: Phase, progress: &GitProgress) {
        self.progress.set_message(format!(
            "{} {repository} ({})",
            phase.message(),
            format_git_progress(progress)
        ));
    }

    fn phase_finished(&mut self, _repository: &str, _phase: Phase) {
        self.progress.set_message("");
    }
}

fn format_git_progress(progress: &GitProgress) -> String {
    let mut message = progress.phase().to_string();
    if let Some(percent) = progress.percent() {
        message.push_str(&format!(" {percent}%"));
    }
    match (progress.current(), progress.total()) {
        (Some(current), Some(total)) => message.push_str(&format!(", {current}/{total}")),
        (Some(current), None) => message.push_str(&format!(", {current}")),
        _ => {}
    }
    message
}
