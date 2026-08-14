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

    // Load image — must be RGBA (4 channels)
    let img = image::open(&args.input)
        .with_context(|| format!("Failed to open image: {:?}", args.input))?;

    // If the image has no alpha channel it cannot carry a watermark
    let rgba = match img {
        image::DynamicImage::ImageRgba8(rgba) => rgba,
        other => {
            // Convert; if original was RGB/grayscale alpha will be all 255 → absent
            other.to_rgba8()
        }
    };

    let (w, h) = rgba.dimensions();
    let pixels = rgba.as_raw();
    let metrics = SignalMetrics::compute(pixels, w, h);

    let present = metrics.is_present(args.threshold);

    if present {
        println!("WATERMARK PRESENT");
    } else {
        println!("WATERMARK ABSENT OR DESTROYED");
    }

    if args.verbose {
        println!("  image:     {}×{} ({} pixels)", w, h, metrics.total_pixels);
        println!("  {}", metrics.summary());
        println!("  threshold: {}", args.threshold);
    }

    Ok(present)
}
