//! Microsoft PowerPoint COM automation for Windows.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result, bail};

use super::ConversionRequest;

const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$inputPath = $env:PPT2IMAGES_INPUT
$outputDirectory = $env:PPT2IMAGES_OUTPUT
$width = [int]$env:PPT2IMAGES_WIDTH
$height = [int]$env:PPT2IMAGES_HEIGHT
$powerPoint = $null
$presentation = $null
try {
    $powerPoint = New-Object -ComObject PowerPoint.Application
    $presentation = $powerPoint.Presentations.Open($inputPath, -1, 0, 0)
    $count = $presentation.Slides.Count
    for ($index = 1; $index -le $count; $index++) {
        $name = 'slide-{0:D4}.png' -f ($index - 1)
        $path = Join-Path $outputDirectory $name
        $presentation.Slides.Item($index).Export($path, 'PNG', $width, $height)
    }
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
} finally {
    if ($null -ne $presentation) {
        try { $presentation.Close() } catch { }
    }
    if ($null -ne $powerPoint) {
        try { $powerPoint.Quit() } catch { }
    }
}
"#;

/// Converts through Microsoft PowerPoint's COM automation interface.
pub fn convert(request: &ConversionRequest) -> Result<Vec<PathBuf>> {
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

    let output = run_script(
        &executable,
        &input,
        &export_directory,
        request.resolution.width,
        request.resolution.height,
    )?;
    if !output.status.success() {
        bail!(
            "Microsoft PowerPoint failed to export '{}': {}",
            request.input.display(),
            command_diagnostic(&output)
        );
    }

    let exported = collect_exports(&export_directory)?;
    let mut writer = request.output.writer(exported.len(), request.resolution)?;
    for (index, path) in exported.iter().enumerate() {
        let bytes = fs::read(path)
            .with_context(|| format!("failed to read exported image '{}'", path.display()))?;
        writer
            .write_page(index, &bytes)
            .with_context(|| format!("failed to write slide {}", index + 1))?;
    }
    writer.finish()
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
    width: u32,
    height: u32,
) -> Result<Output> {
    let mut child = Command::new(executable)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "-",
        ])
        .env("PPT2IMAGES_INPUT", input)
        .env("PPT2IMAGES_OUTPUT", output_directory)
        .env("PPT2IMAGES_WIDTH", width.to_string())
        .env("PPT2IMAGES_HEIGHT", height.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start '{}'", executable.display()))?;
    let mut stdin = child
        .stdin
        .take()
        .context("failed to open PowerShell standard input")?;
    stdin
        .write_all(SCRIPT.as_bytes())
        .context("failed to send the PowerPoint automation script to PowerShell")?;
    drop(stdin);
    child
        .wait_with_output()
        .context("failed to wait for the PowerPoint automation script")
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
            .is_none_or(|name| name != expected.as_str())
        {
            bail!(
                "PowerPoint produced an unexpected export set; expected '{expected}' but found '{}'",
                path.display()
            );
        }
    }
    Ok(files)
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
    use std::fs;

    use tempfile::tempdir;

    use super::collect_exports;

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
}
