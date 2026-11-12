//! Output size selection and aspect-ratio-aware resolution derivation.

/// The largest width or height accepted for one output image.
pub const MAX_DIMENSION: u32 = 16_384;
/// The largest pixel count accepted for one output image.
pub const MAX_PIXELS: u64 = 67_108_864;
/// The largest DPI value accepted on the command line.
pub const MAX_DPI: u32 = 2_400;
/// The exact size used when no size option is supplied.
pub const DEFAULT_RESOLUTION: Resolution = Resolution {
    width: 1920,
    height: 1080,
};

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

/// Physical dimensions of the source slide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceSize {
    width_inches: f64,
    height_inches: f64,
}

impl SourceSize {
    /// Creates source dimensions from inches.
    pub fn from_inches(width_inches: f64, height_inches: f64) -> Result<Self, String> {
        if !width_inches.is_finite()
            || !height_inches.is_finite()
            || width_inches <= 0.0
            || height_inches <= 0.0
        {
            return Err("slide dimensions must be finite and greater than zero".to_owned());
        }
        Ok(Self {
            width_inches,
            height_inches,
        })
    }

    /// Creates source dimensions from CSS pixels at 96 DPI.
    pub fn from_css_pixels(width: f64, height: f64) -> Result<Self, String> {
        Self::from_inches(width / 96.0, height / 96.0)
    }

    /// Creates source dimensions from PostScript points at 72 DPI.
    pub fn from_points(width: f64, height: f64) -> Result<Self, String> {
        Self::from_inches(width / 72.0, height / 72.0)
    }

    /// Width divided by height.
    pub fn aspect_ratio(self) -> f64 {
        self.width_inches / self.height_inches
    }

    /// Width in inches.
    pub const fn width_inches(self) -> f64 {
        self.width_inches
    }

    /// Height in inches.
    pub const fn height_inches(self) -> f64 {
        self.height_inches
    }
}

/// How the output image size is selected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SizeSpec {
    /// Exact width and height.
    Exact(Resolution),
    /// Width in pixels; the height follows the slide aspect ratio.
    Width(u32),
    /// Height in pixels; the width follows the slide aspect ratio.
    Height(u32),
    /// Dots per inch based on the slide's physical dimensions.
    Dpi(u32),
}

impl SizeSpec {
    /// Computes the exact output dimensions for a source slide.
    pub fn resolve(self, source: SourceSize) -> Result<Resolution, String> {
        match self {
            Self::Exact(resolution) => Ok(resolution),
            Self::Width(width) => {
                let height = (f64::from(width) / source.aspect_ratio()).round();
                Resolution::new(width, rounded_dimension(height)?)
            }
            Self::Height(height) => {
                let width = (f64::from(height) * source.aspect_ratio()).round();
                Resolution::new(rounded_dimension(width)?, height)
            }
            Self::Dpi(dpi) => Resolution::new(
                rounded_dimension(source.width_inches() * f64::from(dpi))?,
                rounded_dimension(source.height_inches() * f64::from(dpi))?,
            ),
        }
    }

    /// A short description for status output.
    pub fn describe(self) -> String {
        match self {
            Self::Exact(resolution) => resolution.to_string(),
            Self::Width(width) => format!("{width}px wide, auto height"),
            Self::Height(height) => format!("{height}px high, auto width"),
            Self::Dpi(dpi) => format!("{dpi} DPI"),
        }
    }
}

impl Default for SizeSpec {
    fn default() -> Self {
        Self::Exact(DEFAULT_RESOLUTION)
    }
}

fn rounded_dimension(value: f64) -> Result<u32, String> {
    if !value.is_finite() || value <= 0.0 {
        return Err("computed output dimension is not a positive finite value".to_owned());
    }
    let rounded = value.round();
    if rounded > f64::from(u32::MAX) {
        return Err("computed output dimension is too large".to_owned());
    }
    Ok((rounded as u32).max(1))
}

/// Parses a positive pixel dimension.
pub fn parse_dimension(value: &str) -> Result<u32, String> {
    let dimension = value
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("invalid pixel dimension '{value}'"))?;
    if dimension == 0 {
        return Err("pixel dimension must be greater than zero".to_owned());
    }
    if dimension > MAX_DIMENSION {
        return Err(format!("pixel dimension must not exceed {MAX_DIMENSION}"));
    }
    Ok(dimension)
}

/// Parses a positive DPI value.
pub fn parse_dpi(value: &str) -> Result<u32, String> {
    let dpi = value
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("invalid DPI value '{value}'"))?;
    if !(1..=MAX_DPI).contains(&dpi) {
        return Err(format!("DPI must be between 1 and {MAX_DPI}"));
    }
    Ok(dpi)
}

#[cfg(test)]
mod tests {
    use super::{MAX_DIMENSION, Resolution, SizeSpec, SourceSize, parse_dimension, parse_dpi};

    fn widescreen() -> SourceSize {
        SourceSize::from_inches(13.333_333, 7.5).unwrap()
    }

    #[test]
    fn parses_exact_resolutions() {
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
    fn parses_dimensions_and_dpi() {
        assert_eq!(parse_dimension(" 1920 ").unwrap(), 1920);
        assert!(parse_dimension("0").is_err());
        assert!(parse_dimension("1920.5").is_err());
        assert!(parse_dimension(&(MAX_DIMENSION + 1).to_string()).is_err());
        assert_eq!(parse_dpi("300").unwrap(), 300);
        assert!(parse_dpi("0").is_err());
        assert!(parse_dpi("2401").is_err());
    }

    #[test]
    fn exact_size_is_used_unchanged() {
        let resolution = SizeSpec::Exact(Resolution {
            width: 800,
            height: 600,
        })
        .resolve(widescreen())
        .unwrap();
        assert_eq!(
            resolution,
            Resolution {
                width: 800,
                height: 600
            }
        );
    }

    #[test]
    fn width_only_preserves_aspect_ratio() {
        let resolution = SizeSpec::Width(1920).resolve(widescreen()).unwrap();
        assert_eq!(
            resolution,
            Resolution {
                width: 1920,
                height: 1080
            }
        );

        let standard = SourceSize::from_inches(10.0, 7.5).unwrap();
        let resolution = SizeSpec::Width(1000).resolve(standard).unwrap();
        assert_eq!(
            resolution,
            Resolution {
                width: 1000,
                height: 750
            }
        );
    }

    #[test]
    fn height_only_preserves_aspect_ratio() {
        let resolution = SizeSpec::Height(1080).resolve(widescreen()).unwrap();
        assert_eq!(
            resolution,
            Resolution {
                width: 1920,
                height: 1080
            }
        );

        let standard = SourceSize::from_inches(10.0, 7.5).unwrap();
        let resolution = SizeSpec::Height(750).resolve(standard).unwrap();
        assert_eq!(
            resolution,
            Resolution {
                width: 1000,
                height: 750
            }
        );
    }

    #[test]
    fn dpi_uses_physical_slide_size() {
        let resolution = SizeSpec::Dpi(96).resolve(widescreen()).unwrap();
        assert_eq!(
            resolution,
            Resolution {
                width: 1280,
                height: 720
            }
        );

        let resolution = SizeSpec::Dpi(150).resolve(widescreen()).unwrap();
        assert_eq!(
            resolution,
            Resolution {
                width: 2000,
                height: 1125
            }
        );
    }

    #[test]
    fn derived_sizes_stay_within_limits() {
        assert!(SizeSpec::Dpi(2_400).resolve(widescreen()).is_err());
        assert!(
            SizeSpec::Width(MAX_DIMENSION)
                .resolve(SourceSize::from_inches(10.0, 20.0).unwrap())
                .is_err()
        );
    }

    #[test]
    fn rejects_invalid_source_sizes() {
        assert!(SourceSize::from_inches(0.0, 7.5).is_err());
        assert!(SourceSize::from_inches(f64::NAN, 7.5).is_err());
        assert!(SourceSize::from_points(-10.0, 10.0).is_err());
    }
}
