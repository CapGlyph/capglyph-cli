use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::info;

use crate::cli::{EmbedArgs, EmbedMode};
use crate::geometry::{AnalysisParams, GeometryFile, PathEntry};

/// Entry point for the `embed` subcommand.
pub fn run(args: &EmbedArgs) -> Result<()> {
    embed(args)
}

/// Core embed logic (also called by batch module).
pub fn embed(args: &EmbedArgs) -> Result<()> {
    // ── 1. Resolve output path ────────────────────────────────────────────────
    let default_ext = if args
        .output
        .as_ref()
        .map(|p| p.extension().and_then(|e| e.to_str()))
        == Some(Some("jpg"))
    {
        "jpg"
    } else {
        "png"
    };
    let output_path = resolve_output(&args.input, args.output.as_deref(), "_sigil", default_ext);

    // ── 2. Load the source image ──────────────────────────────────────────────
    info!("Loading input image: {:?}", args.input);
    let src_img = image::open(&args.input)
        .with_context(|| format!("Failed to open input image: {:?}", args.input))?;
    let (orig_w, orig_h) = (src_img.width(), src_img.height());

    // ── 3. Obtain geometry (analyse or load from file) ────────────────────────
    let geometry = if let Some(ref geo_path) = args.from_geometry {
        info!("Loading geometry from: {:?}", geo_path);
        let bytes = std::fs::read(geo_path)
            .with_context(|| format!("Failed to read geometry file: {:?}", geo_path))?;
        GeometryFile::from_json(&bytes)?
    } else {
        info!(
            "Analysing image geometry (detail={}, min_path_len={}, chaikin_iters={})",
            args.detail, args.min_path_len, args.chaikin_iters
        );
        extract_geometry(&src_img, orig_w, orig_h, args)?
    };

    // ── 3a. Warn if path count is very low ────────────────────────────────────
    let path_count = geometry.paths.len();
    if path_count < 50 {
        eprintln!(
            "⚠️  WARNING: Only {} paths extracted. Watermark may be weak or easily removed.",
            path_count
        );
        eprintln!("    Consider lowering --detail or --min-path-len for richer geometry.");
    }

    // ── 4. Optionally persist geometry ────────────────────────────────────────
    if let Some(ref save_path) = args.save_geometry {
        info!("Saving geometry to: {:?}", save_path);
        let json = geometry.to_json()?;
        std::fs::write(save_path, &json)
            .with_context(|| format!("Failed to write geometry file: {:?}", save_path))?;
    }

    // ── 5. Embed via selected mode ────────────────────────────────────────────
    match args.mode {
        EmbedMode::Alpha | EmbedMode::Dct | EmbedMode::Dwt => {
            let (out_img, block_info) = embed_to_image(
                &src_img,
                args.mode,
                &geometry,
                args.stroke,
                args.recipient_id.as_deref(),
                args.key.as_deref(),
                &args.placement,
            )?;

            // Persist block/position coordinates to the geometry file when a
            // recipient-id was embedded (mirrors the old per-mode behaviour).
            if args.recipient_id.is_some() {
                if let (Some((_, blocks)), Some(ref save_path)) = (&block_info, &args.save_geometry)
                {
                    let mut geometry_with_blocks = geometry.clone();
                    geometry_with_blocks.blocks = Some(blocks.clone());
                    let json = geometry_with_blocks.to_json()?;
                    std::fs::write(save_path, &json).with_context(|| {
                        format!("Failed to update geometry file: {:?}", save_path)
                    })?;
                }
            }

            let rid_note = args
                .recipient_id
                .as_deref()
                .map(|id| format!(" recipient={id}"))
                .unwrap_or_default();
            let count = block_info.as_ref().map(|(n, _)| *n).unwrap_or(0);

            if output_path.extension().and_then(|e| e.to_str()) == Some("jpg") {
                let rgb = match args.mode {
                    EmbedMode::Alpha => out_img.to_rgb8(),
                    _ => {
                        let rgba = out_img.to_rgba8();
                        let alphas: Vec<u8> = rgba.pixels().map(|p| p[3]).collect();
                        composite_rgb_over_white(&out_img.to_rgb8(), &alphas, orig_w, orig_h)
                    }
                };
                save_as_jpeg(&rgb, &output_path, 85)?;
                println!(
                    "Watermark embedded [{}→jpg, {} blocks{}] → {:?}",
                    args.mode, count, rid_note, output_path
                );
            } else {
                out_img
                    .save(&output_path)
                    .with_context(|| format!("Failed to save output: {:?}", output_path))?;
                println!(
                    "Watermark embedded [{}, {} blocks{}] → {:?}",
                    args.mode, count, rid_note, output_path
                );
            }
        }
        EmbedMode::Learned => {
            #[cfg(not(feature = "learned"))]
            {
                anyhow::bail!(
                    "learned mode requires the `learned` cargo feature (build with --features learned)"
                );
            }
            #[cfg(feature = "learned")]
            {
                info!(
                    "Mode: learned (TrustMark Q/BCH_5)  strength={}",
                    args.strength
                );
                let rid = args.recipient_id.clone().ok_or_else(|| {
                    anyhow::anyhow!("learned mode requires --recipient-id (61-bit payload)")
                })?;
                if rid.len() > 7 {
                    anyhow::bail!(
                        "recipient-id too long for learned mode (BCH_5 = 61 bits ≈ 7 ASCII bytes), got {} bytes",
                        rid.len()
                    );
                }
                let dir = crate::learned::model_dir(args.model_dir.as_deref());
                let seed = crate::learned::image_seed(&src_img.to_rgb8());
                let payload = crate::learned::payload_bits(&rid, args.key.as_deref(), seed);
                let out =
                    crate::learned::embed_bits(src_img.clone(), &payload, &dir, args.strength)?;

                let output = match &args.output {
                    Some(p) => p.clone(),
                    None => resolve_output(&args.input, None, "_sigil", "png"),
                };
                if output.extension().and_then(|e| e.to_str()) == Some("jpg") {
                    out.to_rgb8()
                        .save(&output)
                        .with_context(|| format!("Failed to save JPEG: {:?}", output))?;
                    println!("Watermark embedded [learned→jpg] → {:?}", output);
                } else {
                    out.save(&output)
                        .with_context(|| format!("Failed to save output: {:?}", output))?;
                    println!("Watermark embedded [learned, {rid}] → {:?}", output);
                }
            }
        }
    }

    #[cfg(feature = "c2pa")]
    if args.c2pa {
        use crate::c2pa::WatermarkClaim;
        let cert = args
            .c2pa_cert
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("--c2pa requires --c2pa-cert"))?;
        let pkey = args
            .c2pa_pkey
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("--c2pa requires --c2pa-pkey"))?;
        let output = match &args.output {
            Some(p) => p.clone(),
            None => resolve_output(&args.input, None, "_sigil", "png"),
        };
        // sign_image rejects in-place; sign to a hidden sibling temp file
        // with the SAME extension (c2pa derives the format from the path),
        // then rename into place
        let fname = output
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "out".to_string());
        let tmp = output
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(format!(".c2pa_tmp_{}_{fname}", std::process::id()));
        let claim = WatermarkClaim {
            mode: args.mode.to_string(),
            recipient_id: args.recipient_id.clone(),
            keyed: args.key.is_some(),
        };
        if let Err(e) = crate::c2pa::sign_image(&output, &tmp, cert, pkey, &claim, None, None)
            .with_context(|| "C2PA signing of the watermarked output failed")
        {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        if let Err(e) = std::fs::rename(&tmp, &output)
            .with_context(|| format!("Failed to move C2PA-signed output into place: {output:?}"))
        {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        println!("C2PA manifest signed on output");
    }

    Ok(())
}

/// In-memory embed result: the watermarked image plus the block/position
/// coordinates used for recipient-id extraction (dct/dwt only; `None` for alpha).
pub(crate) type EmbedOutput = (image::DynamicImage, Option<(u64, Vec<(u32, u32)>)>);

/// In-memory embed core shared by the CLI (`embed::embed`) and the wasm byte
/// API (`wasm_api::embed_bytes`). Returns the watermarked RGBA image plus the
/// block/position coordinates used for recipient-id extraction (dct/dwt only;
/// `None` for alpha).
pub(crate) fn embed_to_image(
    img: &image::DynamicImage,
    mode: EmbedMode,
    geometry: &GeometryFile,
    stroke: f32,
    recipient_id: Option<&str>,
    key: Option<&str>,
    placement: &crate::cli::PlacementStrategy,
) -> Result<EmbedOutput> {
    let (orig_w, orig_h) = (img.width(), img.height());

    match mode {
        EmbedMode::Alpha => {
            let src_rgba = img.to_rgba8();
            let wm_layer = render_watermark(geometry, orig_w, orig_h, stroke)?;
            let result = composite(&src_rgba, &wm_layer, orig_w, orig_h);
            Ok((image::DynamicImage::ImageRgba8(result), None))
        }
        EmbedMode::Dct => {
            let orig_alpha: Option<Vec<u8>> = if img.color().has_alpha() {
                Some(img.to_rgba8().pixels().map(|p| p[3]).collect())
            } else {
                None
            };
            let mut rgb = img.to_rgb8();
            let (count, blocks) =
                crate::dct::embed(&mut rgb, geometry, recipient_id, key, placement)?;
            let rgba = match orig_alpha {
                Some(alphas) => merge_rgb_alpha(&rgb, &alphas, orig_w, orig_h),
                None => image::DynamicImage::ImageRgb8(rgb).to_rgba8(),
            };
            Ok((image::DynamicImage::ImageRgba8(rgba), Some((count, blocks))))
        }
        EmbedMode::Dwt => {
            let orig_alpha: Option<Vec<u8>> = if img.color().has_alpha() {
                Some(img.to_rgba8().pixels().map(|p| p[3]).collect())
            } else {
                None
            };
            let mut rgb = img.to_rgb8();
            let (n_coeffs, dwt_positions) =
                crate::dwt_embed::embed(&mut rgb, geometry, recipient_id, key, placement)?;
            let rgba = match orig_alpha {
                Some(alphas) => merge_rgb_alpha(&rgb, &alphas, orig_w, orig_h),
                None => image::DynamicImage::ImageRgb8(rgb).to_rgba8(),
            };
            Ok((
                image::DynamicImage::ImageRgba8(rgba),
                Some((n_coeffs, dwt_positions)),
            ))
        }
        EmbedMode::Learned => anyhow::bail!(
            "learned mode is not supported in the in-memory embed API (requires ONNX Runtime)"
        ),
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Geometry extraction parameters.
pub struct GeometryParams {
    pub detail: u8,
    pub min_path_len: usize,
    pub chaikin_iters: usize,
    pub color: bool,
    pub recipient_id: Option<String>,
}

/// Public wrapper for geometry extraction (used by info command).
pub fn extract_and_build_geometry(
    rgb: &image::RgbImage,
    width: u32,
    height: u32,
    params: &GeometryParams,
) -> Result<GeometryFile> {
    // Convert RgbImage to DynamicImage for extract_geometry
    let dyn_img = image::DynamicImage::ImageRgb8(rgb.clone());

    // Build temporary EmbedArgs
    let args = EmbedArgs {
        input: std::path::PathBuf::from("dummy.png"),
        output: None,
        mode: crate::cli::EmbedMode::Dct,
        placement: Default::default(),
        stroke: 0.010,
        detail: params.detail,
        min_path_len: params.min_path_len,
        chaikin_iters: params.chaikin_iters,
        color: params.color,
        save_geometry: None,
        from_geometry: None,
        recipient_id: params.recipient_id.clone(),
        key: None,
        model_dir: None,
        strength: 0.95,
        #[cfg(feature = "c2pa")]
        c2pa: false,
        #[cfg(feature = "c2pa")]
        c2pa_cert: None,
        #[cfg(feature = "c2pa")]
        c2pa_pkey: None,
    };

    extract_geometry(&dyn_img, width, height, &args)
}

/// Run the vectomancy-raster pipeline and apply Chaikin smoothing.
fn extract_geometry(
    src: &image::DynamicImage,
    width: u32,
    height: u32,
    args: &EmbedArgs,
) -> Result<GeometryFile> {
    use vectomancy_geometry::{chaikin_smooth_points, simplify_rdp};
    use vectomancy_raster::decode_raster_memory;

    let tolerance = {
        let d = args.detail.clamp(1, 100) as f64;
        5.0 * (1.0 - d / 100.0).powi(2) + 0.1
    };

    let bytes = {
        let mut buf = Vec::new();
        src.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .context("Failed to encode image for raster pipeline")?;
        buf
    };

    let (raw_paths, _dims) = decode_raster_memory(&bytes, args.color)
        .map_err(|e| anyhow::anyhow!("Raster decode failed: {}", e))?;

    let mut paths: Vec<PathEntry> = Vec::new();
    for sp in raw_paths {
        if sp.geometry.points.len() < args.min_path_len {
            continue;
        }
        let simplified = simplify_rdp(&sp.geometry.points, tolerance);
        if simplified.len() < 2 {
            continue;
        }
        let smoothed = if args.chaikin_iters > 0 {
            chaikin_smooth_points(&simplified, args.chaikin_iters, false)
        } else {
            simplified
        };

        let color = sp.color_style.as_deref().and_then(parse_hex_color);
        paths.push(PathEntry {
            color,
            points: smoothed.iter().map(|p| [p.x, p.y]).collect(),
        });
    }

    info!("Extracted {} paths from image", paths.len());

    // If no paths extracted (solid color), compute PRNG seed from source image
    let prng_seed = if paths.is_empty() {
        let rgb = src.to_rgb8();
        Some(crate::dct::image_seed(&rgb))
    } else {
        None
    };

    Ok(GeometryFile {
        version: GeometryFile::CURRENT_VERSION,
        original_width: width,
        original_height: height,
        analysis_params: AnalysisParams {
            detail: args.detail,
            min_path_len: args.min_path_len,
            chaikin_iters: args.chaikin_iters,
            color: args.color,
        },
        paths,
        prng_seed,
        blocks: None,
    })
}

/// Render geometry paths onto a transparent RGBA canvas.
///
/// Uses sparse Bresenham pixel marking instead of continuous stroke AA, mimicking
/// the wgpu 4×MSAA sparse-coverage behaviour:
///   - Only path centerline pixels are marked (Bresenham rasterisation)
///   - Coverage is sub-sampled to ~0.05% of total pixels → isolated scattered dots
///   - Each marked pixel gets a high embed_alpha (proportional to stroke_width)
///   - Result: sparse, isolated pixels invisible to humans; detectable by VLMs
///
/// wgpu reference:    α_nonzero≈0.033%, α_max≈193, MAE≈0.017  (at 0.010px)
/// This renderer:     α_nonzero≈0.04%,  α_max≈180, MAE≈0.020  (at 0.010px, target)
fn render_watermark(
    geometry: &GeometryFile,
    width: u32,
    height: u32,
    stroke_width: f32,
) -> Result<image::RgbaImage> {
    // embed_alpha: high value (like wgpu's ~193) so individual pixels are detectable
    // scale proportional to stroke_width so the user can tune signal strength
    let embed_alpha = (stroke_width * 18_000.0).round().clamp(30.0, 220.0) as u8;

    // Target coverage ≈ 0.05% of pixels (similar to vectomancy wgpu at 0.005–0.010px)
    let total_pixels = width as u64 * height as u64;
    let target_marked: u64 = (total_pixels as f64 * 0.0005).round().max(100.0) as u64;

    let neutral_gray = [0.5f32, 0.5, 0.5];

    // ── Step 1: rasterise all paths via Bresenham, collect (x, y, r, g, b) ─────
    let mut path_pixels: Vec<(i32, i32, u8, u8, u8)> = Vec::new();

    for path_entry in &geometry.paths {
        if path_entry.points.len() < 2 {
            continue;
        }
        let [r, g, b] = path_entry.color.unwrap_or(neutral_gray);
        let pr = (r * 255.0).round().clamp(0.0, 255.0) as u8;
        let pg = (g * 255.0).round().clamp(0.0, 255.0) as u8;
        let pb = (b * 255.0).round().clamp(0.0, 255.0) as u8;

        for win in path_entry.points.windows(2) {
            let (x0, y0) = (win[0][0].round() as i32, win[0][1].round() as i32);
            let (x1, y1) = (win[1][0].round() as i32, win[1][1].round() as i32);
            bresenham(x0, y0, x1, y1, width as i32, height as i32, |x, y| {
                path_pixels.push((x, y, pr, pg, pb));
            });
        }
    }

    // ── Step 2: sub-sample to hit the coverage target ────────────────────────
    let mut pixmap = image::RgbaImage::new(width, height);

    if path_pixels.is_empty() {
        return Ok(pixmap);
    }

    // Uniform stride: keep every N-th pixel
    let stride = ((path_pixels.len() as u64).max(1) as f64 / target_marked as f64)
        .round()
        .max(1.0) as usize;

    for (i, &(x, y, pr, pg, pb)) in path_pixels.iter().enumerate() {
        if i % stride == 0 {
            pixmap.put_pixel(x as u32, y as u32, image::Rgba([pr, pg, pb, embed_alpha]));
        }
    }

    Ok(pixmap)
}

/// Bresenham integer line rasterisation.
/// Calls `emit(x, y)` for every pixel on the segment [(x0,y0)..(x1,y1)]
/// that falls within [0, max_x) × [0, max_y).
fn bresenham<F>(x0: i32, y0: i32, x1: i32, y1: i32, max_x: i32, max_y: i32, mut emit: F)
where
    F: FnMut(i32, i32),
{
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1i32 } else { -1 };
    let sy = if y0 < y1 { 1i32 } else { -1 };
    let mut err = dx - dy;
    let (mut x, mut y) = (x0, y0);

    loop {
        if x >= 0 && y >= 0 && x < max_x && y < max_y {
            emit(x, y);
        }
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x += sx;
        }
        if e2 < dx {
            err += dx;
            y += sy;
        }
    }
}

/// Alpha-composite `watermark` OVER `base`.
/// For each pixel: out_rgb = wm_alpha * wm_rgb + (1 - wm_alpha) * base_rgb
///                 out_alpha = base_alpha (preserve original alpha if any)
fn composite(
    base: &image::RgbaImage,
    watermark: &image::RgbaImage,
    width: u32,
    height: u32,
) -> image::RgbaImage {
    assert_eq!(base.dimensions(), (width, height));
    assert_eq!(watermark.dimensions(), (width, height));

    let mut out = image::RgbaImage::new(width, height);
    for (x, y, out_px) in out.enumerate_pixels_mut() {
        let b = base.get_pixel(x, y);
        let w = watermark.get_pixel(x, y);
        let wa = w[3] as f32 / 255.0;
        let composite_r = (wa * w[0] as f32 + (1.0 - wa) * b[0] as f32).round() as u8;
        let composite_g = (wa * w[1] as f32 + (1.0 - wa) * b[1] as f32).round() as u8;
        let composite_b = (wa * w[2] as f32 + (1.0 - wa) * b[2] as f32).round() as u8;
        // Alpha: path pixels carry the signal (embed_alpha < 255 = semi-transparent).
        // Non-path pixels (w[3]==0) stay at original alpha (255 for RGB sources).
        // verify detects the watermark by counting semi-transparent pixels (0 < α < 255).
        // PNG→JPG drops the alpha channel entirely → all pixels become fully opaque → absent.
        let composite_a = if w[3] > 0 { w[3] } else { b[3] };
        *out_px = image::Rgba([composite_r, composite_g, composite_b, composite_a]);
    }
    out
}

/// Parse a CSS hex color string like `#rrggbb` → `[r, g, b]` in 0.0–1.0.
fn parse_hex_color(s: &str) -> Option<[f32; 3]> {
    let s = s.trim().strip_prefix('#')?;
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
    let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
    let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
    Some([r, g, b])
}

/// Build a default output path: `<parent>/<stem><suffix>.<ext>`
pub fn resolve_output(input: &Path, override_: Option<&Path>, suffix: &str, ext: &str) -> PathBuf {
    if let Some(p) = override_ {
        return p.to_path_buf();
    }
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    let parent = input.parent().unwrap_or(Path::new("."));
    parent.join(format!("{}{}.{}", stem, suffix, ext))
}

/// Reconstruct an RGBA image from separate RGB data and a flat alpha vec.
fn merge_rgb_alpha(rgb: &image::RgbImage, alphas: &[u8], w: u32, h: u32) -> image::RgbaImage {
    image::RgbaImage::from_fn(w, h, |x, y| {
        let p = rgb.get_pixel(x, y);
        let a = alphas[(y * w + x) as usize];
        image::Rgba([p[0], p[1], p[2], a])
    })
}

/// Composite RGB + alpha over a white background → flat RGB (for JPEG output).
fn composite_rgb_over_white(
    rgb: &image::RgbImage,
    alphas: &[u8],
    w: u32,
    h: u32,
) -> image::RgbImage {
    image::RgbImage::from_fn(w, h, |x, y| {
        let p = rgb.get_pixel(x, y);
        let a = alphas[(y * w + x) as usize] as f32 / 255.0;
        let r = (a * p[0] as f32 + (1.0 - a) * 255.0).round() as u8;
        let g = (a * p[1] as f32 + (1.0 - a) * 255.0).round() as u8;
        let b = (a * p[2] as f32 + (1.0 - a) * 255.0).round() as u8;
        image::Rgb([r, g, b])
    })
}

/// Save an RGB image as JPEG with the given quality (10–100).
fn save_as_jpeg(rgb: &image::RgbImage, path: &Path, quality: u8) -> Result<()> {
    let mut file = std::fs::File::create(path)
        .with_context(|| format!("Failed to create JPEG output: {:?}", path))?;
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, quality);
    encoder
        .encode(
            rgb,
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .with_context(|| format!("Failed to encode JPEG: {:?}", path))?;
    Ok(())
}
