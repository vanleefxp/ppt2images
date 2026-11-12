//! Microsoft PowerPoint COM automation for Windows.

use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{Context, Result, bail};

use super::{ConversionOutput, ConversionRequest};
use crate::progress::Progress;
use crate::size::{Resolution, SizeSpec, SourceSize};

const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$inputPath = $env:PPT2IMAGES_INPUT
$outputDirectory = $env:PPT2IMAGES_OUTPUT
$invariant = [System.Globalization.CultureInfo]::InvariantCulture
$powerPoint = $null
$presentation = $null
$failure = $null
try {
    $powerPoint = New-Object -ComObject PowerPoint.Application
    $presentation = $powerPoint.Presentations.Open($inputPath, -1, 0, 0)
    $count = $presentation.Slides.Count
    $slideWidth = ([double]$presentation.PageSetup.SlideWidth).ToString('R', $invariant)
    $slideHeight = ([double]$presentation.PageSetup.SlideHeight).ToString('R', $invariant)
    [Console]::Out.WriteLine('PPT2IMAGES_META ' + $slideWidth + ' ' + $slideHeight + ' ' + $count)
    [Console]::Out.Flush()
    $target = [Console]::In.ReadLine()
    if ([string]::IsNullOrWhiteSpace($target)) {
        throw 'No export dimensions were received from ppt2images.'
    }
    $parts = $target.Trim().Split(' ')
    if ($parts.Count -ne 2) {
        throw 'Invalid export dimensions received from ppt2images.'
    }
    $width = [int]$parts[0]
    $height = [int]$parts[1]
    if ($width -le 0 -or $height -le 0) {
        throw 'Invalid export dimensions received from ppt2images.'
    }
    for ($index = 1; $index -le $count; $index++) {
        $name = 'slide-{0:D4}.png' -f ($index - 1)
        $path = Join-Path $outputDirectory $name
        $presentation.Slides.Item($index).Export($path, 'PNG', $width, $height)
        [Console]::Out.WriteLine('PPT2IMAGES_PROGRESS ' + $index + ' ' + $count)
        [Console]::Out.Flush()
    }
} catch {
    $failure = $_.Exception.Message
    [Console]::Error.WriteLine($failure)
} finally {
    if ($null -ne $presentation) {
        try { $presentation.Close() } catch { }
    }
    if ($null -ne $powerPoint) {
        try { $powerPoint.Quit() } catch { }
    }
}
if ($null -ne $failure) {
    exit 1
}
"#;

#[derive(Clone, Copy, Debug, PartialEq)]
struct ScriptMetadata {
    slide_width_points: f64,
    slide_height_points: f64,
    slide_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ScriptEvent {
    Metadata(ScriptMetadata),
    Progress { completed: usize },
}

struct ScriptRun {
    status: ExitStatus,
    metadata: Option<ScriptMetadata>,
    resolution: Option<Resolution>,
    stderr: String,
    stdout_tail: String,
}

/// Converts through Microsoft PowerPoint's COM automation interface.
pub fn convert(request: &ConversionRequest) -> Result<ConversionOutput> {
    if !cfg!(windows) {
        bail!("the Microsoft PowerPoint backend requires Windows and an installed PowerPoint");
    }
    let executable = find_powershell().context(
        "PowerShell was not found; the Microsoft PowerPoint backend requires Windows PowerShell or PowerShell 7",
    )?;
    let input = request.input.clone();
    let workspace = tempfile::Builder::new()
        .prefix("ppt2images-powerpoint-")
        .tempdir()
        .context("failed to create a PowerPoint workspace")?;
    let export_directory = workspace.path().join("export");
    fs::create_dir_all(&export_directory)
        .context("failed to create the PowerPoint export directory")?;
    let stderr_path = workspace.path().join("powerpoint-stderr.txt");
    let script_path = workspace.path().join("export.ps1");

    let run = run_script(
        &executable,
        &input,
        &export_directory,
        &script_path,
        &stderr_path,
        request.size,
    )?;
    if !run.status.success() {
        bail!(
            "Microsoft PowerPoint failed to export '{}': {}",
            request.input.display(),
            command_diagnostic(&run.status, &run.stderr, &run.stdout_tail)
        );
    }
    let metadata = run
        .metadata
        .context("PowerPoint did not report the presentation's slide size")?;
    if metadata.slide_count == 0 {
        bail!("the presentation contains no slides");
    }
    let resolution = run
        .resolution
        .context("PowerPoint did not receive an output size")?;

    let exported = collect_exports(&export_directory)?;
    if exported.len() != metadata.slide_count {
        bail!(
            "PowerPoint reported {} slides but exported {} images",
            metadata.slide_count,
            exported.len()
        );
    }

    eprintln!("Finalizing {} exported images", exported.len());
    let mut writer = request.output.writer(exported.len(), resolution)?;
    for (index, path) in exported.iter().enumerate() {
        let bytes = fs::read(path)
            .with_context(|| format!("failed to read exported image '{}'", path.display()))?;
        writer
            .write_page(index, &bytes)
            .with_context(|| format!("failed to write slide {}", index + 1))?;
    }
    let files = writer.finish()?;
    Ok(ConversionOutput { files, resolution })
}

fn find_powershell() -> Option<PathBuf> {
    if let Some(configured) = env::var_os("PPT2IMAGES_POWERSHELL") {
        let path = PathBuf::from(configured);
        return path.is_file().then_some(path);
    }
    for name in ["powershell.exe", "pwsh.exe", "powershell", "pwsh"] {
        if let Some(path) = find_on_path(name) {
            return Some(path);
        }
    }
    None
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for directory in env::split_paths(&path) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn run_script(
    executable: &Path,
    input: &Path,
    output_directory: &Path,
    script_path: &Path,
    stderr_path: &Path,
    size: SizeSpec,
) -> Result<ScriptRun> {
    fs::write(script_path, SCRIPT)
        .with_context(|| format!("failed to write '{}'", script_path.display()))?;
    let stderr_file = fs::File::create(stderr_path)
        .with_context(|| format!("failed to create '{}'", stderr_path.display()))?;
    let mut child = Command::new(executable)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script_path)
        .env("PPT2IMAGES_INPUT", input)
        .env("PPT2IMAGES_OUTPUT", output_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .with_context(|| format!("failed to start '{}'", executable.display()))?;
    let mut stdin = child
        .stdin
        .take()
        .context("failed to open PowerShell standard input")?;

    let stdout = child
        .stdout
        .take()
        .context("failed to capture PowerShell standard output")?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let mut metadata = None;
    let mut resolution = None;
    let mut size_error = None;
    let mut progress = None;
    let mut stdout_tail = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .context("failed to read PowerPoint automation progress")?;
        if read == 0 {
            break;
        }
        let text = line.trim_end_matches(['\r', '\n']);
        if let Some(event) = parse_script_event(text) {
            match event {
                ScriptEvent::Metadata(value) => {
                    metadata = Some(value);
                    progress = Some(Progress::new(value.slide_count));
                    let source = SourceSize::from_points(
                        value.slide_width_points,
                        value.slide_height_points,
                    )
                    .map_err(anyhow::Error::msg)
                    .context("PowerPoint reported invalid slide dimensions");
                    let target = source.and_then(|source| {
                        size.resolve(source).map_err(anyhow::Error::msg).context(
                            "failed to derive an output size from the PowerPoint slide dimensions",
                        )
                    });
                    match target {
                        Ok(target) => {
                            writeln!(stdin, "{} {}", target.width, target.height)
                                .context("failed to send export dimensions to PowerPoint")?;
                            stdin
                                .flush()
                                .context("failed to flush export dimensions to PowerPoint")?;
                            resolution = Some(target);
                        }
                        Err(error) => {
                            let _ = writeln!(stdin, "0 0");
                            let _ = stdin.flush();
                            size_error = Some(error);
                        }
                    }
                }
                ScriptEvent::Progress { completed } => {
                    if let Some(progress) = &progress {
                        progress.advance(completed, None);
                    }
                }
            }
        } else if !text.is_empty() {
            if text.starts_with("PPT2IMAGES_META ") {
                let _ = writeln!(stdin, "0 0");
                let _ = stdin.flush();
            }
            append_tail(&mut stdout_tail, text);
        }
    }
    drop(stdin);
    let status = child
        .wait()
        .context("failed to wait for the PowerPoint automation script")?;
    let stderr = fs::read_to_string(stderr_path).unwrap_or_default();
    if let Some(error) = size_error {
        bail!(
            "{error}: {}",
            command_diagnostic(&status, &stderr, &stdout_tail)
        );
    }
    Ok(ScriptRun {
        status,
        metadata,
        resolution,
        stderr,
        stdout_tail,
    })
}

fn parse_script_event(line: &str) -> Option<ScriptEvent> {
    if let Some(rest) = line.strip_prefix("PPT2IMAGES_META ") {
        let mut parts = rest.split_whitespace();
        let width = parts.next()?.parse::<f64>().ok()?;
        let height = parts.next()?.parse::<f64>().ok()?;
        let slide_count = parts.next()?.parse::<usize>().ok()?;
        if parts.next().is_some()
            || !width.is_finite()
            || !height.is_finite()
            || width <= 0.0
            || height <= 0.0
        {
            return None;
        }
        return Some(ScriptEvent::Metadata(ScriptMetadata {
            slide_width_points: width,
            slide_height_points: height,
            slide_count,
        }));
    }
    if let Some(rest) = line.strip_prefix("PPT2IMAGES_PROGRESS ") {
        let mut parts = rest.split_whitespace();
        let completed = parts.next()?.parse::<usize>().ok()?;
        let total = parts.next()?.parse::<usize>().ok()?;
        if parts.next().is_some() || completed == 0 || total == 0 || completed > total {
            return None;
        }
        return Some(ScriptEvent::Progress { completed });
    }
    None
}

fn append_tail(tail: &mut String, line: &str) {
    const MAX_TAIL_BYTES: usize = 8_192;
    if !tail.is_empty() {
        tail.push('\n');
    }
    tail.push_str(line);
    if tail.len() > MAX_TAIL_BYTES {
        let mut start = tail.len() - MAX_TAIL_BYTES;
        while start < tail.len() && !tail.is_char_boundary(start) {
            start += 1;
        }
        tail.drain(..start);
    }
}

fn collect_exports(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut files = fs::read_dir(directory)
        .with_context(|| format!("failed to read '{}'", directory.display()))?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
        })
        .collect::<Vec<_>>();
    files.sort();
    if files.is_empty() {
        bail!("PowerPoint did not export any slides");
    }
    for (index, path) in files.iter().enumerate() {
        let expected = format!("slide-{index:04}.png");
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_none_or(|name| !name.eq_ignore_ascii_case(&expected))
        {
            bail!(
                "PowerPoint produced an unexpected export set; expected '{expected}' but found '{}'",
                path.display()
            );
        }
    }
    Ok(files)
}

fn command_diagnostic(status: &ExitStatus, stderr: &str, stdout_tail: &str) -> String {
    let text = if stderr.trim().is_empty() {
        stdout_tail.trim()
    } else {
        stderr.trim()
    };
    if text.is_empty() {
        format!("process exited with {status}")
    } else {
        text.chars().take(1_000).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{ScriptEvent, ScriptMetadata, collect_exports, parse_script_event};

    #[test]
    fn accepts_contiguous_exports() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("slide-0000.png"), b"a").unwrap();
        fs::write(directory.path().join("slide-0001.png"), b"b").unwrap();
        assert_eq!(collect_exports(directory.path()).unwrap().len(), 2);
    }

    #[test]
    fn rejects_missing_or_unexpected_exports() {
        let directory = tempdir().unwrap();
        assert!(collect_exports(directory.path()).is_err());
        fs::write(directory.path().join("slide-0001.png"), b"b").unwrap();
        assert!(collect_exports(directory.path()).is_err());
    }

    #[test]
    fn parses_metadata_and_progress_events() {
        assert_eq!(
            parse_script_event("PPT2IMAGES_META 960 540 2"),
            Some(ScriptEvent::Metadata(ScriptMetadata {
                slide_width_points: 960.0,
                slide_height_points: 540.0,
                slide_count: 2,
            }))
        );
        assert_eq!(
            parse_script_event("PPT2IMAGES_PROGRESS 1 2"),
            Some(ScriptEvent::Progress { completed: 1 })
        );
    }

    #[test]
    fn rejects_malformed_events() {
        assert!(parse_script_event("PPT2IMAGES_META 960 540").is_none());
        assert!(parse_script_event("PPT2IMAGES_META 0 540 2").is_none());
        assert!(parse_script_event("PPT2IMAGES_PROGRESS 3 2").is_none());
        assert!(parse_script_event("some output").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn exchanges_metadata_and_dimensions_with_the_script() {
        use std::os::unix::fs::PermissionsExt;

        use crate::size::SizeSpec;

        let directory = tempdir().unwrap();
        let fake_powershell = directory.path().join("fake-powershell");
        fs::write(
            &fake_powershell,
            "#!/bin/sh\nprintf 'PPT2IMAGES_META 960 540 2\\n'\nIFS= read -r target\nprintf 'PPT2IMAGES_PROGRESS 1 2\\n'\nprintf 'PPT2IMAGES_PROGRESS 2 2\\n'\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&fake_powershell).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_powershell, permissions).unwrap();

        let output_directory = directory.path().join("export");
        fs::create_dir_all(&output_directory).unwrap();
        let run = super::run_script(
            &fake_powershell,
            &directory.path().join("deck.pptx"),
            &output_directory,
            &directory.path().join("export.ps1"),
            &directory.path().join("stderr.txt"),
            SizeSpec::Width(320),
        )
        .unwrap();
        assert!(run.status.success());
        assert_eq!(
            run.metadata,
            Some(ScriptMetadata {
                slide_width_points: 960.0,
                slide_height_points: 540.0,
                slide_count: 2,
            })
        );
        assert_eq!(
            run.resolution,
            Some(crate::size::Resolution {
                width: 320,
                height: 180,
            })
        );
    }
}
