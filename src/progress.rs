//! Visual slide-export progress reporting.

use std::io::{IsTerminal, stderr};
use std::path::Path;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

const TEMPLATE: &str = "{elapsed_precise} [{wide_bar:.cyan/blue}] {pos}/{len} ({percent}%) {msg}";

/// Draws a live progress bar on an interactive stderr.
///
/// Progress is intentionally hidden when stderr is not a terminal so redirected
/// logs and CI output stay clean.
pub struct Progress {
    bar: ProgressBar,
}

impl Progress {
    /// Creates a reporter for a known slide count.
    pub fn new(total: usize) -> Self {
        let bar = ProgressBar::new(u64::try_from(total).unwrap_or(u64::MAX));
        bar.set_draw_target(if total > 0 && stderr().is_terminal() {
            ProgressDrawTarget::stderr()
        } else {
            ProgressDrawTarget::hidden()
        });
        if let Ok(style) = ProgressStyle::with_template(TEMPLATE) {
            bar.set_style(style.progress_chars("█░"));
        }
        bar.set_message("exporting");
        Self { bar }
    }

    /// Reports one completed slide and the output file being staged.
    pub fn advance(&self, completed: usize, path: Option<&Path>) {
        self.bar
            .set_position(u64::try_from(completed).unwrap_or(u64::MAX));
        if let Some(path) = path {
            self.bar.set_message(path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            ));
        }
    }

    /// Runs output that would otherwise corrupt the active bar.
    pub fn suspend<F, R>(&self, action: F) -> R
    where
        F: FnOnce() -> R,
    {
        self.bar.suspend(action)
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.bar.finish_and_clear();
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Progress;

    #[test]
    fn accepts_zero_total_without_reporting() {
        let progress = Progress::new(0);
        progress.advance(1, Some(Path::new("0000.png")));
    }

    #[test]
    fn reports_completed_work() {
        let progress = Progress::new(2);
        progress.advance(2, Some(Path::new("0001.png")));
        progress.suspend(|| 42);
    }
}
