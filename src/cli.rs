//! Command-line interface definitions.

use std::path::PathBuf;

use clap::{ArgGroup, Parser, ValueEnum};

use crate::page_format::{DEFAULT_PATTERN, PageFormat};
use crate::page_selection::PageSelection;
use crate::size::{Resolution, SizeSpec, parse_dimension, parse_dpi};

/// The largest worker count accepted on the command line.
pub const MAX_JOBS: usize = 64;

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
    group(ArgGroup::new("size").args(["resolution", "width", "height", "dpi"])),
    version,
    about = "Convert PowerPoint slides to numbered PNG images",
    long_about = "Convert a presentation into one PNG per slide.\n\n\
        The native backend is pure Rust and handles PPTX-family packages. \
        The LibreOffice backend uses an installed LibreOffice to produce a PDF \
        and rasterizes it in Rust. The Microsoft PowerPoint backend uses COM \
        automation on Windows. In auto mode, PPTX-family inputs use the native \
        backend and legacy inputs use LibreOffice or PowerPoint.\n\n\
        Output size can be exact (--resolution), driven by one pixel dimension \
        (--width or --height), or based on physical size and DPI (--dpi)."
)]
pub struct Cli {
    /// Presentation to convert (.ppt, .pptx, .pptm, .ppsx, .odp, and related formats).
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,

    /// Directory that receives the numbered PNG files.
    #[arg(short, long, value_name = "DIR", default_value = "output")]
    pub output_dir: PathBuf,

    /// Exact output size in pixels, written as WIDTHxHEIGHT.
    ///
    /// When no size option is supplied, 1920x1080 is used.
    #[arg(
        short,
        long,
        value_name = "WIDTHxHEIGHT",
        value_parser = Resolution::parse
    )]
    pub resolution: Option<Resolution>,

    /// Output width in pixels; the height follows the slide aspect ratio.
    #[arg(long, value_name = "PX", value_parser = parse_dimension)]
    pub width: Option<u32>,

    /// Output height in pixels; the width follows the slide aspect ratio.
    #[arg(long, value_name = "PX", value_parser = parse_dimension)]
    pub height: Option<u32>,

    /// Output dots per inch, based on the slide's physical dimensions.
    #[arg(long, value_name = "DPI", value_parser = parse_dpi)]
    pub dpi: Option<u32>,

    /// Page-number pattern: {page:04}, {:04}, %04d, or the shorthand 0000.
    #[arg(
        long,
        value_name = "FORMAT",
        default_value = DEFAULT_PATTERN,
        value_parser = PageFormat::parse
    )]
    pub page_format: PageFormat,

    /// Number assigned to the first slide; page selection uses this numbering.
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub start_index: u64,

    /// Pages to export, for example 1-3,6,9- or 1-10@2.
    ///
    /// Defaults to all pages. Ranges are inclusive. a- means a through the
    /// last page, -b means the first page through b, and @t selects every
    /// t-th page. Overlapping ranges are merged.
    #[arg(
        long,
        value_name = "RANGES",
        value_parser = PageSelection::parse,
        allow_hyphen_values = true
    )]
    pub pages: Option<PageSelection>,

    /// Conversion backend.
    #[arg(short, long, value_enum, default_value_t = Backend::Auto, ignore_case = true)]
    pub backend: Backend,

    /// Number of parallel page workers; 0 selects an automatic count.
    #[arg(short = 'j', long, value_name = "N", default_value_t = 0, value_parser = parse_jobs)]
    pub jobs: usize,

    /// Replace existing PNG files.
    #[arg(long)]
    pub overwrite: bool,
}

/// Parses a worker count where zero means automatic.
pub fn parse_jobs(value: &str) -> Result<usize, String> {
    let jobs = value
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("invalid worker count '{value}'"))?;
    if jobs > MAX_JOBS {
        return Err(format!("worker count must not exceed {MAX_JOBS}"));
    }
    Ok(jobs)
}

impl Cli {
    /// Resolves the selected size option, defaulting to 1920x1080.
    pub fn size_spec(&self) -> SizeSpec {
        if let Some(resolution) = self.resolution {
            SizeSpec::Exact(resolution)
        } else if let Some(width) = self.width {
            SizeSpec::Width(width)
        } else if let Some(height) = self.height {
            SizeSpec::Height(height)
        } else if let Some(dpi) = self.dpi {
            SizeSpec::Dpi(dpi)
        } else {
            SizeSpec::default()
        }
    }

    /// Returns the selected page ranges, or every page when omitted.
    pub fn selection(&self) -> PageSelection {
        self.pages.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::{Backend, Cli};
    use crate::page_selection::PageSelection;
    use crate::size::{DEFAULT_RESOLUTION, Resolution, SizeSpec};

    #[test]
    fn verifies_cli_shape() {
        Cli::command().debug_assert();
    }

    #[test]
    fn defaults_to_auto_backend_and_exact_size() {
        let cli = Cli::try_parse_from(["ppt2images", "deck.pptx"]).unwrap();
        assert_eq!(cli.backend, Backend::Auto);
        assert_eq!(cli.start_index, 1);
        assert_eq!(cli.size_spec(), SizeSpec::Exact(DEFAULT_RESOLUTION));
        assert_eq!(cli.page_format.format(0), "0000");
        assert_eq!(cli.selection(), PageSelection::all());
        assert_eq!(cli.jobs, 0);
    }

    #[test]
    fn accepts_one_size_selector() {
        let exact = Cli::try_parse_from(["ppt2images", "deck.pptx", "-r", "800x600"]).unwrap();
        assert_eq!(
            exact.size_spec(),
            SizeSpec::Exact(Resolution {
                width: 800,
                height: 600
            })
        );

        let width = Cli::try_parse_from(["ppt2images", "deck.pptx", "--width", "1280"]).unwrap();
        assert_eq!(width.size_spec(), SizeSpec::Width(1280));

        let height = Cli::try_parse_from(["ppt2images", "deck.pptx", "--height", "720"]).unwrap();
        assert_eq!(height.size_spec(), SizeSpec::Height(720));

        let dpi = Cli::try_parse_from(["ppt2images", "deck.pptx", "--dpi", "150"]).unwrap();
        assert_eq!(dpi.size_spec(), SizeSpec::Dpi(150));
    }

    #[test]
    fn parses_page_selections() {
        let cli = Cli::try_parse_from(["ppt2images", "deck.pptx", "--pages", "1-3,6,9-"]).unwrap();
        assert_eq!(cli.selection(), PageSelection::parse("1-3,6,9-").unwrap());
        let open = Cli::try_parse_from(["ppt2images", "deck.pptx", "--pages", "-3"]).unwrap();
        assert_eq!(open.selection(), PageSelection::parse("-3").unwrap());
        let invalid = Cli::try_parse_from(["ppt2images", "deck.pptx", "--pages", "1-2-3"]);
        assert!(invalid.is_err());
    }

    #[test]
    fn parses_worker_counts() {
        let auto = Cli::try_parse_from(["ppt2images", "deck.pptx"]).unwrap();
        assert_eq!(auto.jobs, 0);
        let four = Cli::try_parse_from(["ppt2images", "deck.pptx", "-j", "4"]).unwrap();
        assert_eq!(four.jobs, 4);
        assert!(Cli::try_parse_from(["ppt2images", "deck.pptx", "--jobs", "65"]).is_err());
    }

    #[test]
    fn rejects_conflicting_size_selectors() {
        assert!(
            Cli::try_parse_from([
                "ppt2images",
                "deck.pptx",
                "--resolution",
                "800x600",
                "--width",
                "800",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from(["ppt2images", "deck.pptx", "--width", "800", "--dpi", "96",])
                .is_err()
        );
    }
}
