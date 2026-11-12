//! Supported input formats and extension validation.

use std::path::Path;

use anyhow::{Result, bail};

/// The presentation package family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresentationKind {
    /// OOXML PresentationML such as PPTX, PPTM, PPSX, or POTX.
    OpenXml,
    /// Binary PowerPoint such as PPT, PPS, or POT.
    Legacy,
    /// OpenDocument Presentation.
    OpenDocument,
}

/// Detects the presentation family from the file extension.
pub fn detect_kind(path: &Path) -> Result<PresentationKind> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    let Some(extension) = extension else {
        bail!(
            "input file '{}' has no extension; expected .pptx, .ppt, .ppsx, or another supported presentation format",
            path.display()
        );
    };
    match extension.as_str() {
        "pptx" | "pptm" | "ppsx" | "ppsm" | "potx" | "potm" => Ok(PresentationKind::OpenXml),
        "ppt" | "pps" | "pot" => Ok(PresentationKind::Legacy),
        "odp" => Ok(PresentationKind::OpenDocument),
        _ => bail!(
            "unsupported input extension '.{extension}'; expected .pptx, .pptm, .ppsx, .ppsm, .potx, .potm, .ppt, .pps, .pot, or .odp"
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{PresentationKind, detect_kind};

    #[test]
    fn detects_supported_formats_case_insensitively() {
        assert_eq!(
            detect_kind(Path::new("slides.PPTX")).unwrap(),
            PresentationKind::OpenXml
        );
        assert_eq!(
            detect_kind(Path::new("slides.pptm")).unwrap(),
            PresentationKind::OpenXml
        );
        assert_eq!(
            detect_kind(Path::new("slides.ppt")).unwrap(),
            PresentationKind::Legacy
        );
        assert_eq!(
            detect_kind(Path::new("slides.odp")).unwrap(),
            PresentationKind::OpenDocument
        );
    }

    #[test]
    fn rejects_unknown_extensions() {
        assert!(detect_kind(Path::new("slides.pdf")).is_err());
        assert!(detect_kind(Path::new("slides")).is_err());
    }
}
