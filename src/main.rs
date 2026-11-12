//! `ppt2images` converts presentation slides into numbered PNG images.

mod backend;
mod cli;
mod input;
mod output;
mod page_format;
mod progress;
mod size;

use std::process::ExitCode;

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
        "Wrote {} image{} at {} to '{}'",
        result.files.len(),
        if result.files.len() == 1 { "" } else { "s" },
        result.resolution,
        cli.output_dir.display()
    );
    Ok(())
}
