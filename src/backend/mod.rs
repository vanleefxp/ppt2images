//! Conversion backend selection and dispatch.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::cli::{Backend, Resolution};
use crate::input::{PresentationKind, detect_kind};
use crate::output::OutputSpec;

pub mod fonts;
pub mod libreoffice;
pub mod native;
pub mod powerpoint;

/// Everything a backend needs to convert one presentation.
#[derive(Clone, Debug)]
pub struct ConversionRequest {
    /// Absolute input path.
    pub input: std::path::PathBuf,
    /// Requested output size.
    pub resolution: Resolution,
    /// Output naming and publication options.
    pub output: OutputSpec,
}

/// Converts a presentation with the requested backend.
pub fn convert(backend: Backend, request: &ConversionRequest) -> Result<Vec<std::path::PathBuf>> {
    match backend {
        Backend::Auto => unreachable!("auto backend must be resolved before dispatch"),
        Backend::Native => native::convert(request),
        Backend::Libreoffice => libreoffice::convert(request),
        Backend::Powerpoint => powerpoint::convert(request),
    }
}

/// Resolves a user-requested backend and validates that it is usable.
pub fn resolve(backend: Backend, input: &Path) -> Result<Backend> {
    let kind = detect_kind(input)?;
    match backend {
        Backend::Auto => resolve_auto(kind),
        Backend::Native => {
            ensure_open_xml(kind, Backend::Native)?;
            Ok(Backend::Native)
        }
        Backend::Libreoffice => {
            if libreoffice::find().is_none() {
                bail!(
                    "LibreOffice was not found; install it or set PPT2IMAGES_SOFFICE to the soffice executable"
                );
            }
            Ok(Backend::Libreoffice)
        }
        Backend::Powerpoint => {
            if !cfg!(windows) {
                bail!(
                    "the Microsoft PowerPoint backend requires Windows and an installed PowerPoint"
                );
            }
            Ok(Backend::Powerpoint)
        }
    }
}

fn resolve_auto(kind: PresentationKind) -> Result<Backend> {
    match kind {
        PresentationKind::OpenXml => Ok(Backend::Native),
        PresentationKind::Legacy | PresentationKind::OpenDocument => {
            if libreoffice::find().is_some() {
                return Ok(Backend::Libreoffice);
            }
            if cfg!(windows) {
                return Ok(Backend::Powerpoint);
            }
            bail!(
                "no backend can convert this legacy presentation; install LibreOffice or use --backend powerpoint on Windows"
            )
        }
    }
}

fn ensure_open_xml(kind: PresentationKind, backend: Backend) -> Result<()> {
    if kind == PresentationKind::OpenXml {
        Ok(())
    } else {
        bail!(
            "the {} backend only supports OOXML presentations such as .pptx; use --backend libreoffice for this input",
            backend.display_name()
        )
    }
}

/// Validates the input and converts it to an absolute path.
pub fn prepare_input(path: &Path) -> Result<std::path::PathBuf> {
    if !path.exists() {
        bail!("input file '{}' does not exist", path.display());
    }
    if !path.is_file() {
        bail!("input path '{}' is not a file", path.display());
    }
    std::path::absolute(path)
        .with_context(|| format!("failed to resolve input path '{}'", path.display()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::resolve;
    use crate::cli::Backend;

    #[test]
    fn auto_uses_native_for_ooxml() {
        assert_eq!(
            resolve(Backend::Auto, Path::new("deck.pptx")).unwrap(),
            Backend::Native
        );
    }

    #[test]
    fn native_rejects_legacy_inputs() {
        let error = resolve(Backend::Native, Path::new("deck.ppt")).unwrap_err();
        assert!(error.to_string().contains("only supports OOXML"));
    }
}
