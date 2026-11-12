//! Command-line interface definitions.

use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::page_format::{DEFAULT_PATTERN, PageFormat};

/// The largest width or height accepted for one output image.
pub const MAX_DIMENSION: u32 = 16_384;
/// The largest pixel count accepted for one output image.
pub const MAX_PIXELS: u64 = 67_108_864;

/// Output image dimensions in pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Resolution {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
}

impl Resolution {
    /// Validates and creates a resolution.
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("resolution width and height must be greater than zero".to_owned());
        }
        if width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(format!(
                "resolution must not exceed {MAX_DIMENSION} pixels per side"
            ));
        }
        let pixels = u64::from(width) * u64::from(height);
        if pixels > MAX_PIXELS {
            return Err(format!(
                "resolution contains {pixels} pixels; the maximum is {MAX_PIXELS}"
            ));
        }
        Ok(Self { width, height })
    }

    /// Parses a `WIDTHxHEIGHT` value.
    pub fn parse(value: &str) -> Result<Self, String> {
        let trimmed = value.trim();
        let separator = trimmed
            .char_indices()
            .find(|(_, character)| matches!(character, 'x' | 'X' | '*' | '×'))
            .map(|(index, character)| (index, character.len_utf8()));
        let Some((index, separator_len)) = separator else {
            return Err("resolution must use WIDTHxHEIGHT, for example 1920x1080".to_owned());
        };
        let width_text = trimmed[..index].trim();
        let height_text = trimmed[index + separator_len..].trim();
        if width_text.is_empty() || height_text.is_empty() {
            return Err("resolution must include both width and height".to_owned());
        }
        let width = width_text
            .parse::<u32>()
            .map_err(|_| format!("invalid resolution width '{width_text}'"))?;
        let height = height_text
            .parse::<u32>()
            .map_err(|_| format!("invalid resolution height '{height_text}'"))?;
        Self::new(width, height)
    }
}

impl std::fmt::Display for Resolution {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}x{}", self.width, self.height)
    }
}

/// Conversion engine selected by the user.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Backend {
    /// Pick the best backend for the input on this machine.
    Auto,
    /// Pure Rust PPTX renderer; no office suite required.
    #[value(alias = "rust")]
    Native,
    /// Convert with LibreOffice, then rasterize the resulting PDF in Rust.
    #[value(alias = "lo", alias = "libre-office")]
    Libreoffice,
    /// Drive Microsoft PowerPoint through COM automation (Windows only).
    #[value(alias = "ppt", alias = "microsoft-powerpoint", alias = "ms-powerpoint")]
    Powerpoint,
}

impl Backend {
    /// A short user-facing name.
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Native => "native",
            Self::Libreoffice => "LibreOffice",
            Self::Powerpoint => "Microsoft PowerPoint",
        }
    }
}

/// Convert a PowerPoint presentation into numbered PNG images.
#[derive(Debug, Parser)]
#[command(
    name = "ppt2images",
    version,
    about = "Convert PowerPoint slides to numbered PNG images",
    long_about = "Convert a presentation into one PNG per slide.\n\n\
        The native backend is pure Rust and handles PPTX-family packages. \
        The LibreOffice backend uses an installed LibreOffice to produce a PDF \
        and rasterizes it in Rust. The Microsoft PowerPoint backend uses COM \
        automation on Windows. In auto mode, PPTX-family inputs use the native \
        backend and legacy inputs use LibreOffice or PowerPoint."
)]
pub struct Cli {
    /// Presentation to convert (.ppt, .pptx, .pptm, .ppsx, .odp, and related formats).
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,

    /// Directory that receives the numbered PNG files.
    #[arg(short, long, value_name = "DIR", default_value = "output")]
    pub output_dir: PathBuf,

    /// Output size in pixels, written as WIDTHxHEIGHT.
    #[arg(
        short,
        long,
        value_name = "WIDTHxHEIGHT",
        default_value = "1920x1080",
        value_parser = Resolution::parse
    )]
    pub resolution: Resolution,

    /// Page-number pattern: {page:04}, {:04}, %04d, or the shorthand 0000.
    #[arg(
        long,
        value_name = "FORMAT",
        default_value = DEFAULT_PATTERN,
        value_parser = PageFormat::parse
    )]
    pub page_format: PageFormat,

    /// Number assigned to the first slide.
    #[arg(long, value_name = "N", default_value_t = 0)]
    pub start_index: u64,

    /// Conversion backend.
    #[arg(short, long, value_enum, default_value_t = Backend::Auto, ignore_case = true)]
    pub backend: Backend,

    /// Replace existing PNG files.
    #[arg(long)]
    pub overwrite: bool,
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::{Backend, Cli, MAX_DIMENSION, Resolution};

    #[test]
    fn verifies_cli_shape() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_resolutions() {
        assert_eq!(
            Resolution::parse("1920x1080").unwrap(),
            Resolution {
                width: 1920,
                height: 1080
            }
        );
        assert_eq!(
            Resolution::parse(" 800 X 600 ").unwrap(),
            Resolution {
                width: 800,
                height: 600
            }
        );
        assert_eq!(
            Resolution::parse("3840*2160").unwrap(),
            Resolution {
                width: 3840,
                height: 2160
            }
        );
        assert_eq!(
            Resolution::parse("1280×720").unwrap(),
            Resolution {
                width: 1280,
                height: 720
            }
        );
    }

    #[test]
    fn rejects_invalid_resolutions() {
        assert!(Resolution::parse("1920").is_err());
        assert!(Resolution::parse("x1080").is_err());
        assert!(Resolution::parse("1920x").is_err());
        assert!(Resolution::parse("0x1080").is_err());
        assert!(Resolution::parse("1920x0").is_err());
        assert!(Resolution::parse("1920x1080x2").is_err());
        assert!(Resolution::parse("1920.5x1080").is_err());
        assert!(Resolution::new(MAX_DIMENSION + 1, 100).is_err());
        assert!(Resolution::new(MAX_DIMENSION, MAX_DIMENSION).is_err());
    }

    #[test]
    fn backend_has_expected_default() {
        let cli = Cli::try_parse_from(["ppt2images", "deck.pptx"]).unwrap();
        assert_eq!(cli.backend, Backend::Auto);
        assert_eq!(cli.start_index, 0);
        assert_eq!(
            cli.resolution,
            Resolution {
                width: 1920,
                height: 1080
            }
        );
        assert_eq!(cli.page_format.format(0), "0000");
    }
}
