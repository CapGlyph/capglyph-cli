use crate::cli::{EmbedMode, InfoArgs};
use crate::geometry::GeometryFile;
use crate::signal::SignalMetrics;
use anyhow::Result;
use image::DynamicImage;
use std::path::Path;

/// Show signal statistics without pass/fail verification.
pub fn info(args: &InfoArgs) -> Result<()> {
    let img = image::open(&args.input)?;
    let (w, h) = (img.width(), img.height());

    match args.mode {
        EmbedMode::Learned => {
            anyhow::bail!("info is not supported for learned mode");
        }
        EmbedMode::Alpha => info_alpha(&img, w, h),
        EmbedMode::Dct => info_dct(&img, w, h, args.geometry.as_deref()),
        EmbedMode::Dwt => info_dwt(&img, w, h, args.geometry.as_deref()),
    }
}

fn info_alpha(img: &DynamicImage, w: u32, h: u32) -> Result<()> {
    let rgba = img.to_rgba8();
    let metrics = SignalMetrics::compute(&rgba, w, h);

    println!("Mode:                      alpha");
    println!("Image size:                {}×{} ({} pixels)", w, h, w * h);
    println!("Alpha channel statistics:");
    println!(
        "  Semi-transparent pixels: {} ({:.4}%)",
        metrics.semi_transparent_count,
        metrics.semi_transparent_frac * 100.0
    );
    println!(
        "  Alpha mean/max:          {:.1}/{}",
        metrics.alpha_mean, metrics.alpha_max
    );
    println!("  Nonzero alpha count:     {}", metrics.nonzero_alpha_count);
    println!("  Composite MAE:           {:.3}", metrics.composite_mae);

    if metrics.semi_transparent_frac > 0.001 {
        println!("\n✓ Semi-transparent signal detected (likely watermarked)");
    } else {
        println!("\n✗ No semi-transparent signal (opaque or fully transparent pixels only)");
    }

    Ok(())
}

fn info_dct(img: &DynamicImage, w: u32, h: u32, geometry_path: Option<&Path>) -> Result<()> {
    let rgb = img.to_rgb8();

    let geometry = match geometry_path {
        Some(p) => {
            let json = std::fs::read_to_string(p)?;
            serde_json::from_str::<GeometryFile>(&json)?
        }
        None => {
            tracing::info!("No geometry file provided — re-extracting skeleton from image");
            let params = crate::embed::GeometryParams {
                detail: 60,
                min_path_len: 5,
                chaikin_iters: 3,
                color: false,
                recipient_id: None,
            };
            crate::embed::extract_and_build_geometry(&rgb, w, h, &params)?
        }
    };

    let metrics = crate::dct::verify(&rgb, &geometry)?;

    println!("Mode:                 dct");
    println!("Image size:           {}×{} ({} pixels)", w, h, w * h);
    println!("Geometry paths:       {}", geometry.paths.len());
    println!("Skeleton blocks:      {}", metrics.total_skeleton_blocks);
    println!("Watermarked blocks:   {}", metrics.watermarked_blocks);
    println!("Mean skeleton offset: {:.3}", metrics.mean_offset);
    println!("Mean baseline offset: {:.3}", metrics.baseline_mean_offset);
    println!("Signal strength:      {:.3}", metrics.signal_strength);

    let detection_rate =
        metrics.watermarked_blocks as f64 / metrics.total_skeleton_blocks.max(1) as f64;
    println!("Detection rate:       {:.1}%", detection_rate * 100.0);

    if detection_rate > 0.5 {
        println!("\n✓ DCT watermark signal detected (likely watermarked)");
    } else {
        println!("\n✗ No DCT signal detected");
    }

    Ok(())
}

fn info_dwt(img: &DynamicImage, w: u32, h: u32, geometry_path: Option<&Path>) -> Result<()> {
    let rgb = img.to_rgb8();

    let geometry = match geometry_path {
        Some(p) => {
            let json = std::fs::read_to_string(p)?;
            serde_json::from_str::<GeometryFile>(&json)?
        }
        None => {
            tracing::info!("No geometry file provided — re-extracting skeleton from image");
            let params = crate::embed::GeometryParams {
                detail: 60,
                min_path_len: 5,
                chaikin_iters: 3,
                color: false,
                recipient_id: None,
            };
            crate::embed::extract_and_build_geometry(&rgb, w, h, &params)?
        }
    };

    let metrics = crate::dwt_embed::verify(&rgb, &geometry)?;

    println!("Mode:                 dwt");
    println!("Image size:           {}×{} ({} pixels)", w, h, w * h);
    println!("Geometry paths:       {}", geometry.paths.len());
    println!("Total coefficients:   {}", metrics.total_coefficients);
    println!("Detected coefficients: {}", metrics.detected_count);
    println!("Mean signal:          {:.3}", metrics.mean_signal);
    println!(
        "Detection rate:       {:.1}%",
        metrics.detection_rate * 100.0
    );

    if metrics.detection_rate > 0.5 {
        println!("\n✓ DWT watermark signal detected (likely watermarked)");
    } else {
        println!("\n✗ No DWT signal detected");
    }

    Ok(())
}
