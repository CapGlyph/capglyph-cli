use anyhow::{Context, Result};
use tracing::info;

use crate::cli::VerifyArgs;
use crate::signal::SignalMetrics;

/// Exit codes
pub const EXIT_PRESENT: i32 = 0;
pub const EXIT_ABSENT: i32 = 1;

/// Entry point for the `verify` subcommand.
///
/// Returns `Ok(true)` if watermark is present, `Ok(false)` if absent.
/// The caller is responsible for mapping this to a process exit code.
pub fn run(args: &VerifyArgs) -> Result<bool> {
    info!("Verifying watermark in: {:?}", args.input);

    // Load image — must have an alpha channel to carry a watermark
    let img = image::open(&args.input)
        .with_context(|| format!("Failed to open image: {:?}", args.input))?;

    // If the colour type has no alpha channel the watermark is absent by definition.
    // (strip produces RGB; PNG→JPG conversion also drops alpha.)
    let has_alpha = matches!(
        img.color(),
        image::ColorType::Rgba8
            | image::ColorType::Rgba16
            | image::ColorType::Rgba32F
            | image::ColorType::La8
            | image::ColorType::La16
    );
    if !has_alpha {
        println!("WATERMARK ABSENT OR DESTROYED");
        if args.verbose {
            let (w, h) = (img.width(), img.height());
            println!(
                "  image:  {}×{} — no alpha channel (colour type: {:?})",
                w,
                h,
                img.color()
            );
            println!("  reason: format has no alpha channel (RGB/JPG/stripped)");
        }
        return Ok(false);
    }

    let rgba = img.to_rgba8();

    let (w, h) = rgba.dimensions();
    let pixels = rgba.as_raw();
    let metrics = SignalMetrics::compute(pixels, w, h);

    let present = metrics.is_present(args.threshold);

    if present {
        println!("WATERMARK PRESENT");
        if args.verbose {
            println!(
                "  image:     {}×{} ({} pixels)",
                metrics.width, metrics.height, metrics.total_pixels
            );
            println!("  nonzero:   {} pixels", metrics.nonzero_alpha_count);
            println!("  {}", metrics.summary());
            println!("  threshold: {}", args.threshold);
        }
    } else {
        println!("WATERMARK ABSENT OR DESTROYED");
        if args.verbose {
            println!(
                "  image:     {}×{} ({} pixels)",
                metrics.width, metrics.height, metrics.total_pixels
            );
            println!("  {}", metrics.summary());
            println!("  threshold: {}", args.threshold);
        }
    }

    Ok(present)
}
