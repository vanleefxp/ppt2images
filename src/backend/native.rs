//! Pure-Rust PPTX rendering through `betteroffice-pptx`.

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, bail};
use betteroffice_pptx::{MAX_SLIDE_DIM, MAX_SLIDE_PIXELS, Presentation, RenderOptions};

use super::fonts;
use super::{ConversionOutput, ConversionRequest};
use crate::parallel;
use crate::progress::Progress;
use crate::size::SourceSize;

const EMU_PER_POINT: f64 = 12_700.0;
const CSS_PIXELS_PER_POINT: f64 = 96.0 / 72.0;

/// Renders an OOXML presentation without launching an external application.
pub fn convert(request: &ConversionRequest) -> Result<ConversionOutput> {
    let bytes = fs::read(&request.input)
        .with_context(|| format!("failed to read presentation '{}'", request.input.display()))?;
    let (page_count, width_emu, height_emu) = {
        let presentation = Presentation::open(&bytes).with_context(|| {
            format!(
                "failed to parse OOXML presentation '{}'",
                request.input.display()
            )
        })?;
        let model = presentation.model();
        (
            presentation.slides().len(),
            model.width_emu,
            model.height_emu,
        )
    };
    if page_count == 0 {
        bail!("the presentation contains no slides");
    }

    let plan = request
        .selection
        .resolve(request.output.start_index, page_count)
        .map_err(anyhow::Error::msg)
        .context("invalid page selection")?;
    let base_width =
        emu_to_css_pixels(width_emu).context("the presentation reports an invalid slide width")?;
    let base_height = emu_to_css_pixels(height_emu)
        .context("the presentation reports an invalid slide height")?;
    let source = SourceSize::from_css_pixels(f64::from(base_width), f64::from(base_height))
        .map_err(anyhow::Error::msg)
        .context("the native slide reports invalid physical dimensions")?;
    let resolution = request
        .size
        .resolve(source)
        .map_err(anyhow::Error::msg)
        .context("failed to derive an output size from the native slide dimensions")?;
    let scale = render_scale(base_width, base_height, resolution);
    let fonts = fonts::load_system_fonts()?;
    let writer = request.output.writer(&plan.page_numbers, resolution)?;
    let progress = Progress::new(plan.len());
    let completed = AtomicUsize::new(0);

    parallel::for_each_with(
        request.jobs,
        plan.len(),
        || {
            let mut presentation = Presentation::open(&bytes).with_context(|| {
                format!(
                    "failed to parse OOXML presentation '{}'",
                    request.input.display()
                )
            })?;
            fonts::register_fonts(&mut presentation, &fonts)?;
            Ok(presentation)
        },
        |presentation, output_index| {
            let page_number = plan.page_numbers[output_index];
            let slide_index = plan.slide_indices[output_index];
            let rendered = presentation
                .render_png(
                    slide_index,
                    &RenderOptions {
                        scale,
                        ..RenderOptions::default()
                    },
                )
                .with_context(|| format!("failed to render page {page_number}"))?;
            if rendered.skipped_images > 0 {
                progress.suspend(|| {
                    eprintln!(
                        "warning: page {page_number} skipped {} unsupported or unreadable image(s)",
                        rendered.skipped_images
                    );
                });
            }
            writer
                .write_page(output_index, &rendered.bytes)
                .with_context(|| format!("failed to write page {page_number}"))?;
            let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
            progress.advance(done, writer.page_path(output_index));
            Ok(())
        },
    )?;

    let files = writer.finish()?;
    Ok(ConversionOutput { files, resolution })
}

fn emu_to_css_pixels(emu: i64) -> Result<f32> {
    if emu <= 0 {
        bail!("slide size must be positive");
    }
    let points = (emu as f64 / EMU_PER_POINT).round();
    let pixels = (points * CSS_PIXELS_PER_POINT) as f32;
    if !pixels.is_finite() || pixels <= 0.0 {
        bail!("slide size is not a finite positive value");
    }
    Ok(pixels)
}

/// Chooses a render scale that covers the target size without exceeding the
/// backend's surface limits. Any remaining size difference is handled by the
/// output writer's resampler.
fn render_scale(base_width: f32, base_height: f32, target: crate::size::Resolution) -> f32 {
    let base_width = f64::from(base_width);
    let base_height = f64::from(base_height);
    let desired =
        (f64::from(target.width) / base_width).max(f64::from(target.height) / base_height);
    let max_by_dimension =
        (f64::from(MAX_SLIDE_DIM) / base_width).min(f64::from(MAX_SLIDE_DIM) / base_height);
    let max_by_pixels = (MAX_SLIDE_PIXELS as f64 / (base_width * base_height)).sqrt();
    // The raster backend rounds surface dimensions up, so keep a small margin
    // below the advertised limits instead of landing exactly on them.
    let maximum = (max_by_dimension.min(max_by_pixels) * 0.999).max(1.0);
    desired.clamp(1.0, maximum) as f32
}

#[cfg(test)]
mod tests {
    use super::render_scale;
    use crate::size::Resolution;

    #[test]
    fn render_scale_covers_target_without_upscaling_small_targets() {
        let target = Resolution::new(1920, 1080).unwrap();
        let scale = render_scale(1280.0, 720.0, target);
        assert!((scale - 1.5).abs() < f32::EPSILON);
        assert_eq!(
            render_scale(1280.0, 720.0, Resolution::new(640, 480).unwrap()),
            1.0
        );
    }

    #[test]
    fn render_scale_stays_inside_backend_surface_limits() {
        let scale = render_scale(1280.0, 720.0, Resolution::new(8000, 4500).unwrap());
        let width = (1280.0_f64 * f64::from(scale)).ceil() as u64;
        let height = (720.0_f64 * f64::from(scale)).ceil() as u64;
        assert!(width * height <= betteroffice_pptx::MAX_SLIDE_PIXELS);
        assert!(width <= u64::from(betteroffice_pptx::MAX_SLIDE_DIM));
    }
}
