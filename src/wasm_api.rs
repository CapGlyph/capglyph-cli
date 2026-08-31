//! Byte-in/byte-out watermark API.
//!
//! The wasm bridge (`capglyph-website/wasm-engine`, legacy `sigil-website/wasm-engine`) and any other embedder can
//! call these without touching the filesystem or the CLI types. Everything is
//! in-memory: decode → embed/verify/extract → re-encode.

use anyhow::{Context, Result};

use crate::cli::EmbedMode;

/// Map a mode string ("alpha"|"dct"|"dwt"|"learned") to an `EmbedMode`.
/// `learned` requires the ONNX feature and is rejected here.
fn parse_mode(mode: &str) -> Result<EmbedMode> {
    match mode {
        "alpha" => Ok(EmbedMode::Alpha),
        "dct" => Ok(EmbedMode::Dct),
        "dwt" => Ok(EmbedMode::Dwt),
        "learned" => {
            anyhow::bail!("learned mode is not supported in the byte API (requires ONNX Runtime)")
        }
        other => anyhow::bail!("unknown mode: {other} (expected alpha, dct, or dwt)"),
    }
}

/// Embed a watermark into a decoded PNG/JPEG and return a lossless PNG.
///
/// Geometry is derived in-memory with the CLI defaults pinned for the
/// playground: detail=60, stroke=0.010, min_path_len=5, chaikin_iters=3,
/// color=false.
pub fn embed_bytes(
    src: &[u8],
    mode: &str,
    recipient_id: Option<&str>,
    key: Option<&str>,
) -> Result<Vec<u8>> {
    let mode = parse_mode(mode)?;
    let img = image::load_from_memory(src).context("failed to decode input image")?;

    let geometry = crate::embed::extract_and_build_geometry(
        &img.to_rgb8(),
        img.width(),
        img.height(),
        &crate::embed::GeometryParams {
            detail: 60,
            min_path_len: 5,
            chaikin_iters: 3,
            color: false,
            recipient_id: recipient_id.map(String::from),
        },
    )?;

    let (out_img, _) = crate::embed::embed_to_image(
        &img,
        mode,
        &geometry,
        0.010,
        recipient_id,
        key,
        &crate::cli::PlacementStrategy::Skeleton,
        crate::dwt_embed::DWT_EMBED_STRENGTH,
    )?;

    let mut buf = std::io::Cursor::new(Vec::new());
    out_img
        .write_to(&mut buf, image::ImageFormat::Png)
        .context("failed to encode output PNG")?;
    Ok(buf.into_inner())
}

/// Detect watermark presence in a decoded PNG/JPEG.
///
/// Presence only — no secret-layer key check (that stays in the CLI).
pub fn verify_bytes(src: &[u8], mode: &str) -> Result<bool> {
    let mode = parse_mode(mode)?;
    let img = image::load_from_memory(src).context("failed to decode input image")?;

    match mode {
        EmbedMode::Alpha => {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let metrics = crate::signal::SignalMetrics::compute(rgba.as_raw(), w, h);
            Ok(metrics.is_present(0.0001))
        }
        EmbedMode::Dct => {
            let rgb = img.to_rgb8();
            let geometry = crate::verify::extract_geometry_from_image(&img)?;
            let metrics =
                crate::dct::verify(&rgb, &geometry, &crate::cli::PlacementStrategy::Skeleton)?;
            Ok(metrics.is_present(4.0))
        }
        EmbedMode::Dwt => {
            let rgb = img.to_rgb8();
            let geometry = crate::verify::extract_geometry_from_image(&img)?;
            let metrics = crate::dwt_embed::verify(
                &rgb,
                &geometry,
                &crate::cli::PlacementStrategy::Skeleton,
            )?;
            Ok(metrics.is_present(4.0))
        }
        EmbedMode::Learned => {
            anyhow::bail!("learned mode is not supported in the byte API (requires ONNX Runtime)")
        }
    }
}

/// Extract the embedded recipient-id string from a decoded PNG/JPEG.
///
/// `id_length` is the expected recipient-id length in characters. Returns an
/// error for `alpha` (no recoverable bits) and `learned` (no ONNX in wasm).
pub fn extract_bytes(src: &[u8], mode: &str, id_length: usize) -> Result<String> {
    let mode = parse_mode(mode)?;
    let img = image::load_from_memory(src).context("failed to decode input image")?;
    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();

    match mode {
        EmbedMode::Dct => crate::extract::extract_from_dct(&rgb, id_length, w, h),
        EmbedMode::Dwt => crate::extract::extract_from_dwt(&rgb, id_length, w, h),
        EmbedMode::Alpha => {
            anyhow::bail!("extract is not supported for alpha mode (alpha mode does not carry recoverable bits)")
        }
        EmbedMode::Learned => {
            anyhow::bail!("learned mode is not supported in the byte API (requires ONNX Runtime)")
        }
    }
}
