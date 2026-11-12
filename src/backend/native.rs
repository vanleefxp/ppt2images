//! Pure-Rust PPTX rendering through `betteroffice-pptx`.

use std::fs;

use anyhow::{Context, Result, bail};
use betteroffice_pptx::{MAX_SLIDE_DIM, MAX_SLIDE_PIXELS, Presentation, RenderOptions};

use super::fonts::register_system_fonts;
use super::{ConversionOutput, ConversionRequest};
use crate::progress::Progress;
use crate::size::SourceSize;

/// Renders an OOXML presentation without launching an external application.
pub fn convert(request: &ConversionRequest) -> Result<ConversionOutput> {
    let bytes = fs::read(&request.input)
        .with_context(|| format!("failed to read presentation '{}'", request.input.display()))?;
    let mut presentation = Presentation::open(&bytes).with_context(|| {
        format!(
            "failed to parse OOXML presentation '{}'",
            request.input.display()
        )
    })?;
    let page_count = presentation.slides().len();
    if page_count == 0 {
        bail!("the presentation contains no slides");
    }

    register_system_fonts(&mut presentation)?;

    let first_slide = presentation
        .render_slide(0)
        .context("failed to lay out the first slide")?;
    let base_width = first_slide.display_list.width;
    let base_height = first_slide.display_list.height;
    if !base_width.is_finite()
        || !base_height.is_finite()
        || base_width <= 0.0
        || base_height <= 0.0
    {
        bail!("the presentation reports an invalid slide size");
    }
    drop(first_slide);

    let source = SourceSize::from_css_pixels(f64::from(base_width), f64::from(base_height))
        .map_err(anyhow::Error::msg)
        .context("the native slide reports invalid physical dimensions")?;
    let resolution = request
        .size
        .resolve(source)
        .map_err(anyhow::Error::msg)
        .context("failed to derive an output size from the native slide dimensions")?;
    let scale = render_scale(base_width, base_height, resolution);
    let mut writer = request.output.writer(page_count, resolution)?;
    let progress = Progress::new(page_count);

    for index in 0..page_count {
        let rendered = presentation
            .render_png(
                index,
                &RenderOptions {
                    scale,
                    ..RenderOptions::default()
                },
            )
            .with_context(|| format!("failed to render slide {}", index + 1))?;
        if rendered.skipped_images > 0 {
            progress.suspend(|| {
                eprintln!(
                    "warning: slide {} skipped {} unsupported or unreadable image(s)",
                    index + 1,
                    rendered.skipped_images
                );
            });
        }
        writer
            .write_page(index, &rendered.bytes)
            .with_context(|| format!("failed to write slide {}", index + 1))?;
        progress.advance(index + 1, writer.page_path(index));
    }

    let files = writer.finish()?;
    Ok(ConversionOutput { files, resolution })
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
