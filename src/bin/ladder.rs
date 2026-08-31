//! CTX-0025 ladder: carrier/robust/stealth FER across 512/1024/1254 × dct/dwt × placements × attacks.
//!
//! Reuses framing/ECC (capglyph_core::ecc/framing) and registration (TranslationRegistration)
//! as required by CTX-0025. Measures FER for 128b opaque token (16B payload → sealed 54B → ECC)
//! across the attack ladder: JPEG q30/50/75, blur σ1/2, scale 0.5/0.7, crop/rotate.
//!
//! Each record populates attack.family/severity (fix v2 identity-only bug) via explicit
//! attack_family / attack_severity fields derived from the canonical attack name/params.
//!
//! Output: results.json with per-trial records + summary cells. Designed to be archived
//! as immutable tar.gz and to replace engineering targets in
//! research/media-credential/technology/capacity-robustness-and-threats.md.

use anyhow::{Context, Result};
use capglyph::carrier::{DctCarrier, DwtCarrier};
use capglyph::ecc::Profile;
use capglyph::framing::PayloadType;
use capglyph::geometry::{AnalysisParams, GeometryFile, PathEntry};
use capglyph::keying::KeyMaterial;
use capglyph::placement::Placement;
use capglyph::registration::{Registration, TranslationRegistration};
use image::{DynamicImage, ImageBuffer, Rgb};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

// ── CLI ────────────────────────────────────────────────────────────────────
#[derive(clap::Parser, Debug)]
#[command(name = "ladder", about = "CTX-0025 credential FER ladder")]
struct Cli {
    /// Output directory for results.json + manifest
    #[arg(long, default_value = "ladder-output")]
    output: PathBuf,
    /// Trials per cell
    #[arg(long, default_value_t = 5)]
    trials: usize,
    /// Image sizes to test
    #[arg(long, default_value = "512,1024,1254")]
    sizes: String,
    /// Whether to also run DWT placement arms (DWT currently only Skeleton)
    #[arg(long, default_value_t = false)]
    include_dwt_prng_edge: bool,
    /// Include learned mode (requires trustmark feature + models)
    #[arg(long, default_value_t = false)]
    include_learned: bool,
    /// Quick mode: only identity and jpeg attacks
    #[arg(long, default_value_t = false)]
    quick: bool,
}

// ── Attack definition ──────────────────────────────────────────────────────
#[derive(Debug, Clone)]
struct Attack {
    family: &'static str,
    severity: String,
    name: &'static str,
    params: Value,
}

#[allow(dead_code)]
impl Attack {
    fn to_json(&self) -> Value {
        json!({"name": self.name, "parameters": self.params, "family": self.family, "severity": self.severity})
    }
}

fn ladder_attacks() -> Vec<Attack> {
    vec![
        Attack {
            family: "identity",
            severity: "identity".into(),
            name: "identity",
            params: json!({}),
        },
        Attack {
            family: "jpeg",
            severity: "30".into(),
            name: "jpeg",
            params: json!({"quality": 30, "family":"jpeg","severity":"30"}),
        },
        Attack {
            family: "jpeg",
            severity: "50".into(),
            name: "jpeg",
            params: json!({"quality": 50, "family":"jpeg","severity":"50"}),
        },
        Attack {
            family: "jpeg",
            severity: "75".into(),
            name: "jpeg",
            params: json!({"quality": 75, "family":"jpeg","severity":"75"}),
        },
        Attack {
            family: "blur",
            severity: "1.0".into(),
            name: "blur",
            params: json!({"sigma": 1.0, "family":"blur","severity":"1.0"}),
        },
        Attack {
            family: "blur",
            severity: "2.0".into(),
            name: "blur",
            params: json!({"sigma": 2.0, "family":"blur","severity":"2.0"}),
        },
        Attack {
            family: "resize",
            severity: "0.5".into(),
            name: "resize",
            params: json!({"scale": 0.5, "family":"resize","severity":"0.5"}),
        },
        Attack {
            family: "resize",
            severity: "0.7".into(),
            name: "resize",
            params: json!({"scale": 0.7, "family":"resize","severity":"0.7"}),
        },
        Attack {
            family: "crop",
            severity: "0.90".into(),
            name: "crop",
            params: json!({"retain": 0.90, "offset":"center","family":"crop","severity":"0.90"}),
        },
        Attack {
            family: "crop",
            severity: "0.75".into(),
            name: "crop",
            params: json!({"retain": 0.75, "offset":"center","family":"crop","severity":"0.75"}),
        },
        Attack {
            family: "crop",
            severity: "0.50".into(),
            name: "crop",
            params: json!({"retain": 0.50, "offset":"center","family":"crop","severity":"0.50"}),
        },
        Attack {
            family: "rotate",
            severity: "5".into(),
            name: "rotate",
            params: json!({"degrees": 5.0, "family":"rotate","severity":"5"}),
        },
        Attack {
            family: "rotate",
            severity: "15".into(),
            name: "rotate",
            params: json!({"degrees": 15.0, "family":"rotate","severity":"15"}),
        },
    ]
}
fn filtered_attacks(quick: bool) -> Vec<Attack> {
    let all = ladder_attacks();
    if quick {
        all.into_iter()
            .filter(|a| matches!(a.family, "identity" | "jpeg"))
            .collect()
    } else {
        all
    }
}

// ── Image + geometry helpers ───────────────────────────────────────────────
fn make_image(w: u32, h: u32, trial: usize) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    ImageBuffer::from_fn(w, h, |x, y| {
        let v = ((x as usize * 7 + y as usize * 13 + (x as usize * y as usize) / 3 + trial * 17)
            % 251) as u8;
        let g = ((v as usize + 60 + trial) % 255) as u8;
        let b = ((v as usize + 120 + trial * 2) % 255) as u8;
        Rgb([v, g, b])
    })
}

fn make_geometry(w: u32, h: u32, trial: usize) -> GeometryFile {
    // Deterministically vary the line's y-intercept by trial to avoid exact repetition
    let y_off = (trial as f64 * 11.0) % h as f64;
    let points: Vec<[f64; 2]> = (0..64)
        .map(|i| {
            let x = i as f64 * (w as f64 / 64.0);
            let y = (i as f64 * 3.0 + y_off) % h as f64;
            [x, y]
        })
        .collect();
    GeometryFile {
        version: 1,
        original_width: w,
        original_height: h,
        analysis_params: AnalysisParams {
            detail: 60,
            min_path_len: 5,
            chaikin_iters: 3,
            color: false,
        },
        paths: vec![PathEntry {
            color: None,
            points,
        }],
        prng_seed: None,
        blocks: None,
    }
}

// ── Attack implementations (in-memory) ─────────────────────────────────────
fn apply_attack(
    img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    attack: &Attack,
) -> Result<ImageBuffer<Rgb<u8>, Vec<u8>>> {
    match attack.family {
        "identity" => Ok(img.clone()),
        "jpeg" => {
            let quality = attack.params["quality"].as_u64().unwrap_or(75) as u8;
            jpeg_roundtrip(img, quality)
        }
        "blur" => {
            let sigma = attack.params["sigma"].as_f64().unwrap_or(1.0) as f32;
            Ok(image::imageops::blur(img, sigma))
        }
        "resize" => {
            let scale = attack.params["scale"].as_f64().unwrap_or(0.5) as f32;
            let (w, h) = img.dimensions();
            let nw = ((w as f32 * scale).round() as u32).max(1);
            let nh = ((h as f32 * scale).round() as u32).max(1);
            let small = DynamicImage::ImageRgb8(img.clone())
                .resize_exact(nw, nh, image::imageops::FilterType::Triangle)
                .to_rgb8();
            let restored = DynamicImage::ImageRgb8(small)
                .resize_exact(w, h, image::imageops::FilterType::Triangle)
                .to_rgb8();
            Ok(restored)
        }
        "crop" => {
            let retain = attack.params["retain"].as_f64().unwrap_or(0.9) as f32;
            let (w, h) = img.dimensions();
            let cw = ((w as f32 * retain).round() as u32).max(1);
            let ch = ((h as f32 * retain).round() as u32).max(1);
            let x = (w - cw) / 2;
            let y = (h - ch) / 2;
            let cropped = DynamicImage::ImageRgb8(img.clone())
                .crop_imm(x, y, cw, ch)
                .to_rgb8();
            let restored = DynamicImage::ImageRgb8(cropped)
                .resize_exact(w, h, image::imageops::FilterType::Triangle)
                .to_rgb8();
            Ok(restored)
        }
        "rotate" => {
            let deg = attack.params["degrees"].as_f64().unwrap_or(5.0) as f32;
            Ok(rotate_image(img, deg))
        }
        _ => anyhow::bail!("unknown attack family {}", attack.family),
    }
}

fn jpeg_roundtrip(
    img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    quality: u8,
) -> Result<ImageBuffer<Rgb<u8>, Vec<u8>>> {
    let mut buf = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
    enc.encode_image(&DynamicImage::ImageRgb8(img.clone()))
        .context("jpeg encode failed")?;
    let decoded = image::load_from_memory(&buf)
        .context("jpeg decode failed")?
        .to_rgb8();
    // Ensure dimensions match original (jpeg shouldn't change size, but be safe)
    let (w, h) = img.dimensions();
    if decoded.dimensions() != (w, h) {
        let resized = DynamicImage::ImageRgb8(decoded)
            .resize_exact(w, h, image::imageops::FilterType::Triangle)
            .to_rgb8();
        return Ok(resized);
    }
    Ok(decoded)
}

fn rotate_image(
    img: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    degrees: f32,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let (w, h) = img.dimensions();
    let w_f = w as f32;
    let h_f = h as f32;
    let cx = w_f / 2.0;
    let cy = h_f / 2.0;
    let rad = degrees.to_radians();
    let cos = rad.cos();
    let sin = rad.sin();
    let mut out = ImageBuffer::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let xf = x as f32 - cx;
            let yf = y as f32 - cy;
            // Inverse rotate
            let sx = cos * xf + sin * yf + cx;
            let sy = -sin * xf + cos * yf + cy;
            let pixel = bilinear_sample(img, sx, sy);
            out.put_pixel(x, y, pixel);
        }
    }
    out
}

fn bilinear_sample(img: &ImageBuffer<Rgb<u8>, Vec<u8>>, x: f32, y: f32) -> Rgb<u8> {
    let (w, h) = img.dimensions();
    if x < 0.0 || y < 0.0 || x >= w as f32 || y >= h as f32 {
        return Rgb([128, 128, 128]);
    }
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let p00 = img.get_pixel(x0, y0);
    let p10 = img.get_pixel(x1, y0);
    let p01 = img.get_pixel(x0, y1);
    let p11 = img.get_pixel(x1, y1);
    let lerp = |a: u8, b: u8, t: f32| ((a as f32) * (1.0 - t) + (b as f32) * t).round() as u8;
    let r0 = lerp(p00[0], p10[0], fx);
    let r1 = lerp(p01[0], p11[0], fx);
    let r = lerp(r0, r1, fy);
    let g0 = lerp(p00[1], p10[1], fx);
    let g1 = lerp(p01[1], p11[1], fx);
    let g = lerp(g0, g1, fy);
    let b0 = lerp(p00[2], p10[2], fx);
    let b1 = lerp(p01[2], p11[2], fx);
    let b = lerp(b0, b1, fy);
    Rgb([r, g, b])
}

fn psnr(a: &ImageBuffer<Rgb<u8>, Vec<u8>>, b: &ImageBuffer<Rgb<u8>, Vec<u8>>) -> Option<f64> {
    if a.dimensions() != b.dimensions() {
        return None;
    }
    let mut mse = 0.0f64;
    let n = (a.width() * a.height() * 3) as f64;
    for (pa, pb) in a.pixels().zip(b.pixels()) {
        for c in 0..3 {
            let d = pa[c] as f64 - pb[c] as f64;
            mse += d * d;
        }
    }
    mse /= n;
    if mse == 0.0 {
        return None;
    }
    Some(10.0 * (255.0 * 255.0 / mse).log10())
}

// ── Profile helpers ────────────────────────────────────────────────────────
fn all_profiles() -> Vec<(String, Profile)> {
    vec![
        ("Repetition8".into(), Profile::Repetition8),
        ("Bch{t=3}".into(), Profile::Bch { t: 3 }),
        ("Bch{t=2}".into(), Profile::Bch { t: 2 }),
    ]
}

fn carriers() -> Vec<(&'static str, bool)> {
    vec![("dct", true), ("dwt", true)]
}

fn placements_for(carrier: &str) -> Vec<(&'static str, Placement)> {
    match carrier {
        "dct" => vec![
            ("skeleton", Placement::Skeleton),
            ("prng", Placement::Prng),
            ("edge-density", Placement::Edge),
        ],
        "dwt" => vec![("skeleton", Placement::Skeleton)],
        _ => vec![("skeleton", Placement::Skeleton)],
    }
}

// ── Main runner ────────────────────────────────────────────────────────────
fn main() -> Result<()> {
    use clap::Parser;
    let cli = Cli::parse();
    let sizes: Vec<u32> = cli
        .sizes
        .split(',')
        .map(|s| s.trim().parse().unwrap())
        .collect();
    let attacks = filtered_attacks(cli.quick);
    let profiles = all_profiles();
    let payload: Vec<u8> = (0u8..16).collect(); // 128b
    let keys = KeyMaterial::from_keys([0x11u8; 32], [0x22u8; 32]);

    std::fs::create_dir_all(&cli.output).context("create output dir")?;
    let mut records: Vec<Value> = Vec::new();
    let mut summary_map: HashMap<String, (usize, usize)> = HashMap::new(); // key -> (failures, total)

    let start = std::time::Instant::now();
    for &size in &sizes {
        let w = size;
        let h = size;
        for (carrier_name, _) in carriers() {
            for (placement_name, placement) in placements_for(carrier_name) {
                for (profile_name, profile) in &profiles {
                    for trial in 0..cli.trials {
                        let image_id = format!("synthetic-{}-{:02}-{}", size, trial, carrier_name);
                        let stratum = "synthetic";
                        let original = make_image(w, h, trial);
                        let geometry = make_geometry(w, h, trial);
                        let mut marked = original.clone();
                        // Embed once per trial per cell (reuse across attacks)
                        let embed_res: Result<(u64, Vec<(u32, u32)>)> = match carrier_name {
                            "dct" => DctCarrier::embed_framed(
                                &mut marked,
                                &geometry,
                                &payload,
                                &keys,
                                &placement,
                                *profile,
                                PayloadType::Credential,
                            ),
                            "dwt" => DwtCarrier::embed_framed(
                                &mut marked,
                                &geometry,
                                &payload,
                                &keys,
                                &placement,
                                *profile,
                                PayloadType::Credential,
                            ),
                            _ => Err(anyhow::anyhow!("unknown carrier")),
                        };
                        let (embed_ok, embed_err) = match embed_res {
                            Ok((_, _)) => (true, None),
                            Err(e) => (false, Some(format!("{:#}", e))),
                        };
                        let psnr_val = if embed_ok {
                            psnr(&original, &marked)
                        } else {
                            None
                        };
                        // If embed failed, record once per attack as insufficient_geometry
                        if !embed_ok {
                            for attack in &attacks {
                                let is_capacity =
                                    embed_err.as_deref().unwrap_or("").contains("insufficient");
                                let status = if is_capacity {
                                    "insufficient_geometry"
                                } else {
                                    "failed"
                                };
                                let rec = json!({
                                    "protocol_version": "v2",
                                    "image_id": image_id,
                                    "stratum": stratum,
                                    "mode": carrier_name,
                                    "placement": placement_name,
                                    "carrier": carrier_name,
                                    "profile": profile_name,
                                    "size": size,
                                    "width": w,
                                    "height": h,
                                    "trial": trial,
                                    "attack": {"name": attack.name, "parameters": attack.params, "family": attack.family, "severity": attack.severity},
                                    "attack_family": attack.family,
                                    "attack_severity": attack.severity,
                                    "verification_condition": "blind",
                                    "status": status,
                                    "error": embed_err,
                                    "fer": 1,
                                    "psnr": psnr_val,
                                    "payload_len": payload.len(),
                                });
                                records.push(rec);
                                let key = format!(
                                    "{}|{}|{}|{}|{}|{}",
                                    size,
                                    carrier_name,
                                    placement_name,
                                    profile_name,
                                    attack.family,
                                    attack.severity
                                );
                                let e = summary_map.entry(key).or_insert((0, 0));
                                e.0 += 1;
                                e.1 += 1;
                            }
                            continue;
                        }
                        // For each attack, apply to the same marked image
                        for attack in &attacks {
                            // Attack
                            let attacked = match apply_attack(&marked, attack) {
                                Ok(img) => img,
                                Err(e) => {
                                    let rec = json!({
                                        "protocol_version":"v2",
                                        "image_id": image_id,
                                        "stratum": stratum,
                                        "mode": carrier_name,
                                        "placement": placement_name,
                                        "carrier": carrier_name,
                                        "profile": profile_name,
                                        "size": size,
                                        "trial": trial,
                                        "attack": {"name": attack.name, "parameters": attack.params, "family": attack.family, "severity": attack.severity},
                                        "attack_family": attack.family,
                                        "attack_severity": attack.severity,
                                        "verification_condition":"blind",
                                        "status":"failed",
                                        "error": format!("attack failed: {:#}", e),
                                        "fer":1
                                    });
                                    records.push(rec);
                                    let key = format!(
                                        "{}|{}|{}|{}|{}|{}",
                                        size,
                                        carrier_name,
                                        placement_name,
                                        profile_name,
                                        attack.family,
                                        attack.severity
                                    );
                                    let e2 = summary_map.entry(key).or_insert((0, 0));
                                    e2.0 += 1;
                                    e2.1 += 1;
                                    continue;
                                }
                            };
                            // Blind extract
                            let blind_res: Result<Vec<u8>> = match carrier_name {
                                "dct" => DctCarrier::extract_framed(&attacked, &keys, *profile),
                                "dwt" => DwtCarrier::extract_framed(&attacked, &keys, *profile),
                                _ => Err(anyhow::anyhow!("unknown")),
                            };
                            let blind_ok =
                                blind_res.as_ref().map(|v| v == &payload).unwrap_or(false);
                            let blind_err = blind_res.err().map(|e| format!("{:#}", e));
                            let fer = if blind_ok { 0 } else { 1 };
                            // Also try registered residual for geometric attacks (crop/rotate/scale)
                            let (registered_ok, reg_err) =
                                if matches!(attack.family, "crop" | "rotate" | "resize") {
                                    let reg: Box<dyn Registration> = if attack.family == "rotate" {
                                        Box::new(TranslationRegistration { max_shift: 64 })
                                    } else {
                                        Box::new(TranslationRegistration::default())
                                    };
                                    let rres: Result<Vec<u8>> = match carrier_name {
                                        "dct" => DctCarrier::extract_framed_registered(
                                            &original,
                                            &attacked,
                                            reg.as_ref(),
                                            &keys,
                                            *profile,
                                            Some(16),
                                        ),
                                        "dwt" => DwtCarrier::extract_framed_registered(
                                            &original,
                                            &attacked,
                                            reg.as_ref(),
                                            &keys,
                                            *profile,
                                            Some(16),
                                        ),
                                        _ => Err(anyhow::anyhow!("unknown")),
                                    };
                                    match rres {
                                        Ok(v) => (v == payload, None),
                                        Err(e) => (false, Some(format!("{:#}", e))),
                                    }
                                } else {
                                    (false, None)
                                };
                            // Decide status blind
                            let status = if blind_ok {
                                "completed"
                            } else if blind_err.as_deref().unwrap_or("").contains("insufficient") {
                                "insufficient_geometry"
                            } else {
                                "completed"
                            };
                            // For ladder we still consider status completed but fer=1 if decode fails
                            let rec = json!({
                                "protocol_version":"v2",
                                "image_id": image_id,
                                "stratum": stratum,
                                "mode": carrier_name,
                                "placement": placement_name,
                                "carrier": carrier_name,
                                "profile": profile_name,
                                "size": size,
                                "width": w,
                                "height": h,
                                "trial": trial,
                                "attack": {"name": attack.name, "parameters": attack.params, "family": attack.family, "severity": attack.severity},
                                "attack_family": attack.family,
                                "attack_severity": attack.severity,
                                "verification_condition":"blind",
                                "status": status,
                                "blind_ok": blind_ok,
                                "blind_error": blind_err,
                                "registered_ok": registered_ok,
                                "registered_error": reg_err,
                                "fer": fer,
                                "psnr": psnr_val,
                                "payload_len": payload.len(),
                            });
                            records.push(rec);
                            let key = format!(
                                "{}|{}|{}|{}|{}|{}",
                                size,
                                carrier_name,
                                placement_name,
                                profile_name,
                                attack.family,
                                attack.severity
                            );
                            let e = summary_map.entry(key).or_insert((0, 0));
                            if fer == 1 {
                                e.0 += 1;
                            }
                            e.1 += 1;

                            // Also push registered record for geometric attacks as separate verification_condition
                            if matches!(attack.family, "crop" | "rotate" | "resize") {
                                let reg_fer = if registered_ok { 0 } else { 1 };
                                let reg_rec = json!({
                                    "protocol_version":"v2",
                                    "image_id": image_id,
                                    "stratum": stratum,
                                    "mode": carrier_name,
                                    "placement": placement_name,
                                    "carrier": carrier_name,
                                    "profile": profile_name,
                                    "size": size,
                                    "trial": trial,
                                    "attack": {"name": attack.name, "parameters": attack.params, "family": attack.family, "severity": attack.severity},
                                    "attack_family": attack.family,
                                    "attack_severity": attack.severity,
                                    "verification_condition":"registered",
                                    "status":"completed",
                                    "fer": reg_fer,
                                    "registered_ok": registered_ok,
                                    "blind_ok": blind_ok,
                                });
                                records.push(reg_rec);
                            }
                        }
                    }
                }
            }
        }
    }
    // Build summary cells
    let mut cells: Vec<Value> = Vec::new();
    for (key, (failures, total)) in &summary_map {
        let parts: Vec<&str> = key.split('|').collect();
        let fer = *failures as f64 / *total as f64;
        cells.push(json!({
            "size": parts[0].parse::<u32>().unwrap_or(0),
            "carrier": parts[1],
            "placement": parts[2],
            "profile": parts[3],
            "attack_family": parts[4],
            "attack_severity": parts[5],
            "trials": total,
            "failures": failures,
            "fer": fer,
        }));
    }
    cells.sort_by(|a, b| {
        let sa = a["size"].as_u64().unwrap();
        let sb = b["size"].as_u64().unwrap();
        if sa != sb {
            return sa.cmp(&sb);
        }
        let ca = a["carrier"].as_str().unwrap();
        let cb = b["carrier"].as_str().unwrap();
        if ca != cb {
            return ca.cmp(cb);
        }
        let pa = a["placement"].as_str().unwrap();
        let pb = b["placement"].as_str().unwrap();
        if pa != pb {
            return pa.cmp(pb);
        }
        let fa = a["attack_family"].as_str().unwrap();
        let fb = b["attack_family"].as_str().unwrap();
        if fa != fb {
            return fa.cmp(fb);
        }
        a["attack_severity"]
            .as_str()
            .unwrap()
            .cmp(b["attack_severity"].as_str().unwrap())
    });
    let output_json = json!({
        "protocol_version":"ladder-v1",
        "experiment_id":"ctx-0025-credential-fer-ladder",
        "generated_at": format!("{:?}", std::time::SystemTime::now()),
        "parameters": {
            "sizes": sizes,
            "attacks": filtered_attacks(cli.quick).iter().map(|a| json!({"family":a.family,"severity":a.severity,"name":a.name,"params":a.params})).collect::<Vec<_>>(),
            "profiles": profiles.iter().map(|(n,_)| n).collect::<Vec<_>>(),
            "trials": cli.trials,
            "payload_bytes": payload.len(),
            "payload_bits": payload.len()*8,
            "framing": "CBOR + HMAC-SHA256 (6B header +32B tag=38B overhead → sealed 54B for 16B)",
            "key_material": "KeyMaterial::from_keys([0x11;32],[0x22;32])",
            "note": "FER = frame error rate (1 = HMAC or ECC fail). PSNR is marked vs original. Registered uses TranslationRegistration NCC.",
        },
        "summary": {"cells": cells, "total_records": records.len(), "duration_secs": start.elapsed().as_secs_f64()},
        "records": records,
    });
    let out_path = cli.output.join("results.json");
    std::fs::write(
        &out_path,
        serde_json::to_string_pretty(&output_json).unwrap(),
    )
    .context("write results.json")?;
    println!("wrote {} records to {}", records.len(), out_path.display());
    for cell in &cells {
        println!(
            "{:.0} {} {} {} {} {} FER {:.2} ({}/{})",
            cell["size"].as_u64().unwrap(),
            cell["carrier"].as_str().unwrap(),
            cell["placement"].as_str().unwrap(),
            cell["profile"].as_str().unwrap(),
            cell["attack_family"].as_str().unwrap(),
            cell["attack_severity"].as_str().unwrap(),
            cell["fer"].as_f64().unwrap(),
            cell["failures"].as_u64().unwrap(),
            cell["trials"].as_u64().unwrap()
        );
    }
    Ok(())
}
