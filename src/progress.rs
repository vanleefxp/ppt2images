//! Slide export progress reporting.

use std::path::Path;

/// Reports completed slides as each output image is published to staging.
pub struct Progress {
    total: usize,
}

impl Progress {
    /// Creates a reporter for a known slide count.
    pub fn new(total: usize) -> Self {
        Self { total }
    }

    /// Reports one completed slide.
    pub fn advance(&self, completed: usize, path: Option<&Path>) {
        if self.total == 0 {
            return;
        }
        let completed = completed.min(self.total);
        let percent = (u64::try_from(completed).unwrap_or(u64::MAX) * 100)
            / u64::try_from(self.total).unwrap_or(1);
        match path {
            Some(path) => eprintln!(
                "Exported {completed}/{} ({percent}%): {}",
                self.total,
                path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned()
                )
            ),
            None => eprintln!("Exported {completed}/{} ({percent}%)", self.total),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Progress;

    #[test]
    fn accepts_zero_total_without_reporting() {
        Progress::new(0).advance(1, Some(Path::new("0000.png")));
    }

    #[test]
    fn clamps_progress_to_total() {
        Progress::new(2).advance(9, None);
    }
}
