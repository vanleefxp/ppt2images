//! `ppt2images` converts presentation slides into numbered PNG images.

mod backend;
mod cli;
mod input;
mod output;
mod page_format;
mod progress;
mod size;

use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;

use crate::backend::ConversionRequest;
use crate::cli::Cli;
use crate::output::OutputSpec;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let started = Instant::now();
    let cli = Cli::parse();
    let input = backend::prepare_input(&cli.input)?;
    let backend = backend::resolve(cli.backend, &input)?;
    let size = cli.size_spec();
    let output = OutputSpec {
        directory: cli.output_dir.clone(),
        page_format: cli.page_format.clone(),
        start_index: cli.start_index,
        overwrite: cli.overwrite,
    };
    let request = ConversionRequest {
        input: input.clone(),
        size,
        output,
    };

    eprintln!(
        "Converting '{}' with {} (size: {}){}",
        input.display(),
        backend.display_name(),
        size.describe(),
        if cli.overwrite {
            " (overwrite enabled)"
        } else {
            ""
        }
    );

    let result = backend::convert(backend, &request)
        .with_context(|| format!("conversion of '{}' failed", input.display()))?;
    println!(
        "Wrote {} image{} at {} to '{}' in {}",
        result.files.len(),
        if result.files.len() == 1 { "" } else { "s" },
        result.resolution,
        cli.output_dir.display(),
        format_elapsed(started.elapsed())
    );
    Ok(())
}

/// Formats elapsed time for the final conversion summary.
fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs_f64();
    if seconds < 1.0 {
        return format!("{}ms", elapsed.as_millis());
    }
    if seconds < 60.0 {
        return format!("{seconds:.2}s");
    }
    let total_seconds = elapsed.as_secs();
    let minutes = total_seconds / 60;
    let remainder = seconds - (minutes as f64 * 60.0);
    if minutes < 60 {
        return format!("{minutes}m {remainder:.2}s");
    }
    let hours = minutes / 60;
    let minutes = minutes % 60;
    format!("{hours}h {minutes}m {remainder:.2}s")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::format_elapsed;

    #[test]
    fn formats_sub_second_durations() {
        assert_eq!(format_elapsed(Duration::from_millis(0)), "0ms");
        assert_eq!(format_elapsed(Duration::from_millis(999)), "999ms");
    }

    #[test]
    fn formats_seconds_minutes_and_hours() {
        assert_eq!(format_elapsed(Duration::from_millis(1_234)), "1.23s");
        assert_eq!(format_elapsed(Duration::from_millis(65_500)), "1m 5.50s");
        assert_eq!(
            format_elapsed(Duration::from_millis(3_661_200)),
            "1h 1m 1.20s"
        );
    }
}
