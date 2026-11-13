//! Output naming, PNG normalization, and staged file publication.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageEncoder, ImageFormat, RgbImage};
use tempfile::{Builder, TempDir};

use crate::page_format::PageFormat;
use crate::size::Resolution;

/// User-selected output options shared by all backends.
#[derive(Clone, Debug)]
pub struct OutputSpec {
    /// Destination directory.
    pub directory: PathBuf,
    /// Page-number pattern.
    pub page_format: PageFormat,
    /// Number assigned to the first slide.
    pub start_index: u64,
    /// Whether existing files may be replaced.
    pub overwrite: bool,
}

impl OutputSpec {
    /// Creates a staged writer for the selected page numbers.
    pub fn writer(&self, page_numbers: &[u64], resolution: Resolution) -> Result<OutputWriter> {
        OutputWriter::new(self, page_numbers, resolution)
    }
}

/// Writes each page into a temporary staging directory and publishes the whole
/// set only after every page has been rendered successfully.
pub struct OutputWriter {
    destinations: Vec<PathBuf>,
    resolution: Resolution,
    overwrite: bool,
    staging: TempDir,
    written: Vec<AtomicBool>,
}

impl OutputWriter {
    fn new(spec: &OutputSpec, page_numbers: &[u64], resolution: Resolution) -> Result<Self> {
        if page_numbers.is_empty() {
            bail!("the page selection contains no pages");
        }

        fs::create_dir_all(&spec.directory).with_context(|| {
            format!(
                "failed to create output directory '{}'",
                spec.directory.display()
            )
        })?;
        if !spec.directory.is_dir() {
            bail!(
                "output path '{}' is not a directory",
                spec.directory.display()
            );
        }

        let mut names = BTreeSet::new();
        let mut destinations = Vec::with_capacity(page_numbers.len());
        for page_number in page_numbers {
            let stem = spec.page_format.format(*page_number);
            if !names.insert(stem.clone()) {
                bail!("page format produces duplicate output name '{stem}.png'");
            }
            destinations.push(spec.directory.join(format!("{stem}.png")));
        }

        if let Some(directory) = destinations.iter().find(|path| path.is_dir()) {
            bail!("output path '{}' is a directory", directory.display());
        }
        if !spec.overwrite {
            let conflicts: Vec<&Path> = destinations
                .iter()
                .map(PathBuf::as_path)
                .filter(|path| path.exists())
                .collect();
            if !conflicts.is_empty() {
                let first = conflicts[0].display();
                if conflicts.len() == 1 {
                    bail!("output file '{first}' already exists; pass --overwrite to replace it");
                }
                bail!(
                    "{} output files already exist (first: '{first}'); pass --overwrite to replace them",
                    conflicts.len()
                );
            }
        }

        let staging = Builder::new()
            .prefix(".ppt2images-")
            .tempdir_in(&spec.directory)
            .with_context(|| {
                format!(
                    "failed to create staging directory inside '{}'",
                    spec.directory.display()
                )
            })?;

        Ok(Self {
            destinations,
            resolution,
            overwrite: spec.overwrite,
            staging,
            written: (0..page_numbers.len())
                .map(|_| AtomicBool::new(false))
                .collect(),
        })
    }

    /// Returns the final path planned for one zero-based page index.
    pub fn page_path(&self, index: usize) -> Option<&Path> {
        self.destinations.get(index).map(PathBuf::as_path)
    }

    /// Normalizes and stages one rendered PNG. `index` is zero-based.
    pub fn write_page(&self, index: usize, png_bytes: &[u8]) -> Result<()> {
        if index >= self.destinations.len() {
            bail!(
                "backend produced page index {index}, but only {} pages were planned",
                self.destinations.len()
            );
        }
        if self.written[index].swap(true, Ordering::AcqRel) {
            bail!("backend produced page index {index} more than once");
        }
        let write_result = (|| {
            let file_name = self.destinations[index]
                .file_name()
                .context("planned output path has no file name")?;
            let staged_path = self.staging.path().join(file_name);
            let normalized = normalize_png(png_bytes, self.resolution)?;
            fs::write(&staged_path, normalized)
                .with_context(|| format!("failed to stage output page '{}'", staged_path.display()))
        })();
        if write_result.is_err() {
            self.written[index].store(false, Ordering::Release);
        }
        write_result
    }

    /// Publishes every staged page to its final name.
    pub fn finish(self) -> Result<Vec<PathBuf>> {
        if let Some(index) = self
            .written
            .iter()
            .position(|written| !written.load(Ordering::Acquire))
        {
            bail!("backend did not produce output page {index}");
        }
        for (index, destination) in self.destinations.iter().enumerate() {
            if destination.exists() {
                if !self.overwrite {
                    bail!(
                        "output file '{}' appeared during conversion; refusing to replace it",
                        destination.display()
                    );
                }
                if destination.is_dir() {
                    bail!("output path '{}' is a directory", destination.display());
                }
                fs::remove_file(destination).with_context(|| {
                    format!(
                        "failed to replace existing output file '{}'",
                        destination.display()
                    )
                })?;
            }
            let file_name = destination
                .file_name()
                .context("planned output path has no file name")?;
            let staged_path = self.staging.path().join(file_name);
            fs::rename(&staged_path, destination).with_context(|| {
                format!(
                    "failed to publish '{}' to '{}'",
                    staged_path.display(),
                    destination.display()
                )
            })?;
            debug_assert_eq!(self.destinations[index], *destination);
        }
        Ok(self.destinations)
    }
}

fn normalize_png(png_bytes: &[u8], resolution: Resolution) -> Result<Vec<u8>> {
    let image = image::load_from_memory_with_format(png_bytes, ImageFormat::Png)
        .context("backend returned an invalid PNG image")?;
    let rgba = image.to_rgba8();
    let (source_width, source_height) = rgba.dimensions();
    if source_width == 0 || source_height == 0 {
        bail!("backend returned an empty PNG image");
    }

    let mut flattened = RgbImage::new(source_width, source_height);
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let alpha = u16::from(pixel[3]);
        let blend = |channel: u8| {
            let value = u16::from(channel) * alpha + 255 * (255 - alpha);
            ((value + 127) / 255) as u8
        };
        flattened.put_pixel(
            x,
            y,
            image::Rgb([blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]),
        );
    }

    let resized = if (source_width, source_height) == (resolution.width, resolution.height) {
        flattened
    } else {
        image::imageops::resize(
            &flattened,
            resolution.width,
            resolution.height,
            FilterType::Lanczos3,
        )
    };

    let mut encoded = Vec::new();
    PngEncoder::new(&mut encoded)
        .write_image(
            resized.as_raw(),
            resolution.width,
            resolution.height,
            ExtendedColorType::Rgb8,
        )
        .context("failed to encode output PNG")?;
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder, RgbaImage};
    use tempfile::tempdir;

    use super::OutputSpec;
    use crate::page_format::PageFormat;
    use crate::size::Resolution;

    fn sample_png(width: u32, height: u32) -> Vec<u8> {
        let image = RgbaImage::from_pixel(width, height, image::Rgba([12, 34, 56, 255]));
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(image.as_raw(), width, height, ExtendedColorType::Rgba8)
            .unwrap();
        bytes
    }

    fn spec(directory: &std::path::Path) -> OutputSpec {
        OutputSpec {
            directory: directory.to_path_buf(),
            page_format: PageFormat::default(),
            start_index: 1,
            overwrite: false,
        }
    }

    fn pages(count: u64) -> Vec<u64> {
        (1..=count).collect()
    }

    #[test]
    fn publishes_pages_with_default_names_and_exact_size() {
        let directory = tempdir().unwrap();
        let output = spec(directory.path());
        let writer = output
            .writer(&pages(2), Resolution::new(40, 30).unwrap())
            .unwrap();
        writer.write_page(0, &sample_png(20, 20)).unwrap();
        writer.write_page(1, &sample_png(20, 20)).unwrap();
        let files = writer.finish().unwrap();

        assert_eq!(files[0].file_name().unwrap(), "0001.png");
        assert_eq!(files[1].file_name().unwrap(), "0002.png");
        for path in files {
            let image = image::open(path).unwrap().to_rgb8();
            assert_eq!(image.dimensions(), (40, 30));
        }
    }

    #[test]
    fn flattens_transparency_onto_white() {
        let directory = tempdir().unwrap();
        let output = spec(directory.path());
        let writer = output
            .writer(&pages(1), Resolution::new(2, 2).unwrap())
            .unwrap();
        let image = RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 0]));
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(image.as_raw(), 2, 2, ExtendedColorType::Rgba8)
            .unwrap();
        writer.write_page(0, &bytes).unwrap();
        let files = writer.finish().unwrap();
        let image = image::open(&files[0]).unwrap().to_rgb8();
        assert!(image.pixels().all(|pixel| pixel.0 == [255, 255, 255]));
    }

    #[test]
    fn refuses_existing_output_without_overwrite() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("0001.png"), b"existing").unwrap();
        let error = spec(directory.path())
            .writer(&pages(1), Resolution::new(10, 10).unwrap())
            .err()
            .unwrap();
        assert!(error.to_string().contains("--overwrite"));
    }

    #[test]
    fn overwrites_existing_output_when_requested() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("0001.png"), b"existing").unwrap();
        let mut output = spec(directory.path());
        output.overwrite = true;
        let writer = output
            .writer(&pages(1), Resolution::new(10, 10).unwrap())
            .unwrap();
        writer.write_page(0, &sample_png(10, 10)).unwrap();
        let files = writer.finish().unwrap();
        assert!(image::open(&files[0]).is_ok());
    }

    #[test]
    fn incomplete_output_is_not_published() {
        let directory = tempdir().unwrap();
        let output = spec(directory.path());
        let writer = output
            .writer(&pages(2), Resolution::new(10, 10).unwrap())
            .unwrap();
        writer.write_page(0, &sample_png(10, 10)).unwrap();
        let error = writer.finish().err().unwrap();
        assert!(error.to_string().contains("page 1"));
        assert!(!directory.path().join("0001.png").exists());
    }

    #[test]
    fn honors_page_numbers_and_prefix() {
        let directory = tempdir().unwrap();
        let mut output = spec(directory.path());
        output.start_index = 5;
        output.page_format = PageFormat::parse("slide-{page:03}").unwrap();
        let writer = output
            .writer(&[5, 6], Resolution::new(10, 10).unwrap())
            .unwrap();
        writer.write_page(0, &sample_png(10, 10)).unwrap();
        writer.write_page(1, &sample_png(10, 10)).unwrap();
        let files = writer.finish().unwrap();
        assert_eq!(files[0].file_name().unwrap(), "slide-005.png");
        assert_eq!(files[1].file_name().unwrap(), "slide-006.png");
    }

    #[test]
    fn supports_concurrent_page_writes() {
        let directory = tempdir().unwrap();
        let output = spec(directory.path());
        let writer = output
            .writer(&pages(2), Resolution::new(10, 10).unwrap())
            .unwrap();
        std::thread::scope(|scope| {
            scope.spawn(|| writer.write_page(0, &sample_png(10, 10)).unwrap());
            scope.spawn(|| writer.write_page(1, &sample_png(10, 10)).unwrap());
        });
        let files = writer.finish().unwrap();
        assert_eq!(files.len(), 2);
        assert!(image::open(&files[0]).is_ok());
        assert!(image::open(&files[1]).is_ok());
    }

    #[test]
    fn rejects_duplicate_and_out_of_range_pages() {
        let directory = tempdir().unwrap();
        let output = spec(directory.path());
        let writer = output
            .writer(&pages(1), Resolution::new(10, 10).unwrap())
            .unwrap();
        writer.write_page(0, &sample_png(10, 10)).unwrap();
        assert!(writer.write_page(0, &sample_png(10, 10)).is_err());
        assert!(writer.write_page(1, &sample_png(10, 10)).is_err());
    }
}
