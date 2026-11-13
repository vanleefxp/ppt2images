//! LibreOffice conversion followed by pure-Rust PDF rasterization.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, bail};
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};

use super::{ConversionOutput, ConversionRequest};
use crate::parallel;
use crate::progress::Progress;
use crate::size::SourceSize;

/// Finds LibreOffice without starting it.
pub fn find() -> Option<PathBuf> {
    if let Some(configured) = env::var_os("PPT2IMAGES_SOFFICE") {
        let path = PathBuf::from(configured);
        return is_executable(&path).then_some(path);
    }
    for name in ["soffice", "libreoffice"] {
        if let Some(path) = find_on_path(name) {
            return Some(path);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let extensions: Vec<OsString> = if cfg!(windows) {
        env::var_os("PATHEXT")
            .map(|value| {
                value
                    .to_string_lossy()
                    .split(';')
                    .map(|extension| OsString::from(extension.to_ascii_lowercase()))
                    .collect()
            })
            .unwrap_or_else(|| vec![OsString::from(".exe")])
    } else {
        vec![OsString::new()]
    };

    for directory in env::split_paths(&path) {
        for extension in &extensions {
            let mut candidate = directory.join(name);
            if !extension.is_empty() {
                candidate.set_extension(extension.to_string_lossy().trim_start_matches('.'));
            }
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Converts with LibreOffice to PDF and rasterizes every page in Rust.
pub fn convert(request: &ConversionRequest) -> Result<ConversionOutput> {
    let executable = find().context(
        "LibreOffice was not found; install it or set PPT2IMAGES_SOFFICE to the soffice executable",
    )?;
    let input = request.input.clone();
    let workspace = tempfile::Builder::new()
        .prefix("ppt2images-libreoffice-")
        .tempdir()
        .context("failed to create a LibreOffice workspace")?;
    let profile = workspace.path().join("profile");
    fs::create_dir_all(&profile).context("failed to create the LibreOffice profile")?;

    let output = run_libreoffice(&executable, &input, workspace.path(), &profile)?;
    if !output.status.success() {
        bail!(
            "LibreOffice failed to convert '{}': {}",
            request.input.display(),
            command_diagnostic(&output)
        );
    }

    let pdf_path = find_pdf(workspace.path()).with_context(|| {
        format!(
            "LibreOffice did not produce a PDF for '{}': {}",
            request.input.display(),
            command_diagnostic(&output)
        )
    })?;
    let pdf_bytes =
        fs::read(&pdf_path).with_context(|| format!("failed to read '{}'", pdf_path.display()))?;
    render_pdf(&pdf_bytes, request)
}

fn run_libreoffice(
    executable: &Path,
    input: &Path,
    output_directory: &Path,
    profile: &Path,
) -> Result<Output> {
    let profile_url = file_url(profile)?;
    Command::new(executable)
        .arg("--headless")
        .arg("--nologo")
        .arg("--nodefault")
        .arg("--norestore")
        .arg("--nolockcheck")
        .arg("--nofirststartwizard")
        .arg(format!("-env:UserInstallation={profile_url}"))
        .arg("--convert-to")
        .arg("pdf")
        .arg("--outdir")
        .arg(output_directory)
        .arg(input)
        .output()
        .with_context(|| format!("failed to run LibreOffice at '{}'", executable.display()))
}

fn find_pdf(directory: &Path) -> Option<PathBuf> {
    let mut candidates = fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    if candidates.len() == 1 {
        candidates.pop()
    } else {
        None
    }
}

fn render_pdf(bytes: &[u8], request: &ConversionRequest) -> Result<ConversionOutput> {
    let pdf = Pdf::new(bytes.to_vec())
        .map_err(|error| anyhow::anyhow!("LibreOffice produced an unreadable PDF: {error:?}"))?;
    let pages = pdf.pages();
    if pages.is_empty() {
        bail!("LibreOffice produced a PDF with no pages");
    }

    let (first_width, first_height) = pages[0].render_dimensions();
    let source = SourceSize::from_points(f64::from(first_width), f64::from(first_height))
        .map_err(anyhow::Error::msg)
        .context("the PDF reports invalid physical dimensions")?;
    let resolution = request
        .size
        .resolve(source)
        .map_err(anyhow::Error::msg)
        .context("failed to derive an output size from the PDF page dimensions")?;

    let plan = request
        .selection
        .resolve(request.output.start_index, pages.len())
        .map_err(anyhow::Error::msg)
        .context("invalid page selection")?;
    let writer = request.output.writer(&plan, resolution)?;
    let progress = Progress::new(plan.len());
    let completed = AtomicUsize::new(0);

    parallel::for_each_selected(request.jobs, &plan, |slide_index| {
        let page_number = plan
            .page_number(slide_index)
            .context("selected slide index has no page number")?;
        let page = &pages[slide_index];
        let (page_width, page_height) = page.render_dimensions();
        if page_width <= 0.0 || page_height <= 0.0 {
            bail!("PDF page {page_number} has an invalid size");
        }
        let cache = RenderCache::new();
        let interpreter_settings = InterpreterSettings::default();
        let pixmap = hayro::render(
            page,
            &cache,
            &interpreter_settings,
            &RenderSettings::default(),
            &PixmapSettings {
                // Render one extra pixel per axis to avoid a zero-sized surface
                // from floating-point truncation at very small resolutions.
                x_scale: (resolution.width as f32 + 1.0) / page_width,
                y_scale: (resolution.height as f32 + 1.0) / page_height,
                bg_color: WHITE,
            },
        );
        let png = pixmap
            .into_png()
            .with_context(|| format!("failed to encode PDF page {page_number}"))?;
        writer
            .write_page(slide_index, &png)
            .with_context(|| format!("failed to write page {page_number}"))?;
        let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
        progress.advance(done, writer.page_path(slide_index).as_deref());
        Ok(())
    })?;
    let files = writer.finish()?;
    Ok(ConversionOutput { files, resolution })
}

fn file_url(path: &Path) -> Result<String> {
    let text = path
        .to_str()
        .context("LibreOffice profile path is not valid UTF-8")?;
    let normalized = text.replace('\\', "/");
    let prefixed = if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    };
    Ok(percent_encode_url(&prefixed))
}

fn percent_encode_url(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn command_diagnostic(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    if text.is_empty() {
        format!("process exited with {}", output.status)
    } else {
        text.chars().take(1_000).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{file_url, find_on_path, percent_encode_url};

    #[test]
    fn builds_file_urls() {
        assert_eq!(
            file_url(Path::new("/tmp/a b/profile")).unwrap(),
            "file:///tmp/a%20b/profile"
        );
        assert_eq!(
            file_url(Path::new("C:\\Temp\\profile")).unwrap(),
            "file:///C:/Temp/profile"
        );
    }

    #[test]
    fn encodes_non_url_characters() {
        assert_eq!(percent_encode_url("a b#c"), "a%20b%23c");
    }

    #[test]
    fn finds_known_path_programs() {
        assert!(find_on_path("definitely-not-a-real-program-ppt2images").is_none());
    }
}
