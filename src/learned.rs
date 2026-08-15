//! Learned watermark layer (TrustMark ONNX) — optional `learned` feature.
//!
//! TrustMark (Adobe, MIT) embeds a ~40-75 bit payload via a trained CNN
//! encoder/decoder pair. It survives aggressive ordinary edits (JPEG q30,
//! blur σ2, scale 0.5×) that defeat Sigil's classical DCT/DWT layers, but
//! shares the same limit on generative regeneration (img2img) — see
//! findings/2026-08-15-q114-trustmark-vs-attacks.md.
//!
//! Model files are ONNX weights downloaded once from Adobe's CDN into the
//! XDG data directory (see `model_dir`). `sigil fetch-models` pre-downloads.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use trustmark::{Trustmark, Variant, Version};

/// Default model variant: Q (quality/robustness balance, ResNet-50 decoder).
pub const DEFAULT_VARIANT: &str = "Q";

/// TrustMark BCH_5 payload capacity in bits (61 data bits).
pub const BCH5_DATA_BITS: usize = 61;

/// Adobe CDN hosting the ONNX models.
pub const MODEL_CDN: &str = "https://cai-watermark.adobe.net/watermarking/trustmark-models";

/// Model files required for variant Q.
pub const MODEL_FILES: [&str; 2] = ["encoder_Q.onnx", "decoder_Q.onnx"];

/// Resolve the model directory: explicit `--model-dir` wins, then
/// `$SIGIL_MODEL_DIR`, then the XDG data dir.
pub fn model_dir(explicit: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit {
        return p.to_path_buf();
    }
    if let Ok(dir) = std::env::var("SIGIL_MODEL_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(feature = "learned")]
    {
        if let Some(base) = directories::BaseDirs::new() {
            return base.data_dir().join("sigil").join("models");
        }
    }
    PathBuf::from("models")
}

/// Download all model files into the given directory (idempotent).
pub fn fetch_models(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("Failed to create model dir {:?}", dir))?;
    for name in MODEL_FILES {
        let target = dir.join(name);
        if target.exists() && target.metadata().map(|m| m.len() > 0).unwrap_or(false) {
            continue;
        }
        let url = format!("{MODEL_CDN}/{name}");
        let tmp = target.with_extension("part");
        let agent = http_agent()?;
        let mut resp = agent
            .get(&url)
            .call()
            .with_context(|| format!("Failed to download {url}"))?;
        let data = resp
            .body_mut()
            .with_config()
            .limit(128 * 1024 * 1024)
            .read_to_vec()
            .with_context(|| format!("Failed to read body of {url}"))?;
        std::fs::write(&tmp, &data).with_context(|| format!("Failed to write {:?}", tmp))?;
        std::fs::rename(&tmp, &target)
            .with_context(|| format!("Failed to move {:?} into place", tmp))?;
        tracing::info!("downloaded model {name}");
    }
    Ok(())
}

/// Build a ureq agent honoring HTTPS_PROXY/HTTP_PROXY/ALL_PROXY.
///
/// ureq does not read proxy environment variables by default; explicit
/// wiring keeps `sigil fetch-models` working in proxied environments.
fn http_agent() -> Result<ureq::Agent> {
    let mut builder = ureq::Agent::config_builder();
    let proxy = std::env::var("HTTPS_PROXY")
        .or_else(|_| std::env::var("https_proxy"))
        .or_else(|_| std::env::var("HTTP_PROXY"))
        .or_else(|_| std::env::var("http_proxy"))
        .or_else(|_| std::env::var("ALL_PROXY"))
        .or_else(|_| std::env::var("all_proxy"))
        .ok();
    if let Some(p) = proxy {
        if let Ok(proxy) = ureq::Proxy::new(&p) {
            builder = builder.proxy(Some(proxy));
        }
    }
    Ok(builder.build().into())
}

/// Load TrustMark with the given model directory.
pub fn load(dir: &Path) -> Result<Trustmark> {
    for name in MODEL_FILES {
        if !dir.join(name).exists() {
            return Err(anyhow!(
                "model file {name} missing in {:?} — run `sigil fetch-models` first",
                dir
            ));
        }
    }
    Trustmark::new(dir, Variant::Q, Version::Bch5).context("Failed to load TrustMark models")
}

/// Convert an ASCII recipient id into a TrustMark bitstring (0/1 chars).
///
/// Packs the id bytes into bits (MSB first) and zero-pads/truncates to
/// the BCH_5 capacity of 61 bits (7 bytes + 5 bits).
pub fn id_to_bitstring(id: &str) -> String {
    let bytes = id.as_bytes();
    let mut s = String::with_capacity(BCH5_DATA_BITS);
    for b in bytes {
        for i in (0..8).rev() {
            s.push(if (b >> i) & 1 == 1 { '1' } else { '0' });
        }
    }
    s.truncate(BCH5_DATA_BITS);
    while s.len() < BCH5_DATA_BITS {
        s.push('0');
    }
    s
}

/// Compute a watermark-stable image seed for learned mode.
///
/// TrustMark's residual is much stronger than the classical DCT/DWT
/// residuals, so `dct::stable_seed`'s 16×16 pooling flips at quantization
/// boundaries. This variant pools 64×64, extracts ONE bit per cell
/// (`mean >= 128`), then majority-votes bits in fixed groups of 11 into a
/// 32-bit seed. A few cells sitting exactly on the 128 boundary still flip,
/// but a 1-cell flip never overrules the other 10 in its group (measured
/// flip rate ~5/361 cells on a flat-heavy portrait).
pub fn image_seed(img: &image::RgbImage) -> u64 {
    let (w, h) = img.dimensions();
    let cells_x = (w / 64).max(1);
    let cells_y = (h / 64).max(1);
    let mut bits = Vec::with_capacity((cells_x * cells_y) as usize);
    for cy in 0..cells_y {
        for cx in 0..cells_x {
            let mut sum: u64 = 0;
            let mut count: u64 = 0;
            let y0 = cy * 64;
            let y1 = ((cy + 1) * 64).min(h);
            let x0 = cx * 64;
            let x1 = ((cx + 1) * 64).min(w);
            for y in y0..y1.max(y0 + 1) {
                for x in x0..x1.max(x0 + 1) {
                    let p = img.get_pixel(x, y);
                    sum += (p[0] as u64 + p[1] as u64 + p[2] as u64) / 3;
                    count += 1;
                }
            }
            let mean = sum.checked_div(count).unwrap_or(0);
            bits.push(mean >= 128);
        }
    }
    const GROUP: usize = 11;
    const SEED_BITS: usize = 32;
    let mut seed: u64 = 0;
    for i in 0..SEED_BITS {
        let mut ones = 0usize;
        let mut total = 0usize;
        for b in bits.iter().skip(i * GROUP).take(GROUP) {
            if *b {
                ones += 1;
            }
            total += 1;
        }
        if total > 0 && ones * 2 > total {
            seed |= 1 << i;
        }
    }
    seed
}

/// XOR a bitstring against the first N bytes of the keystream.
fn xor_bitstring(bits: &str, keystream: &[u8; 32]) -> String {
    let mut out = String::with_capacity(bits.len());
    for (i, c) in bits.chars().enumerate() {
        let k = (keystream[i / 8] >> (7 - i % 8)) & 1;
        out.push(if (c == '1') ^ (k == 1) { '1' } else { '0' });
    }
    out
}

/// Compute the payload bitstring written into the watermark.
///
/// Without a key: plain id bits. With a key: id bits XOR
/// HMAC(key, image_seed) keystream — the payload is pseudorandom without the
/// key (ID privacy, forgery resistance: an attacker cannot forge a valid
/// keyed payload for a known ID), and recoverable with it.
///
/// The keystream does NOT depend on the id itself — extraction must be able
/// to recover an unknown id with just the key.
pub fn payload_bits(recipient_id: &str, key: Option<&str>, image_seed: u64) -> String {
    let plain = id_to_bitstring(recipient_id);
    match key {
        Some(k) => {
            let ks = crate::keying::keystream_bytes(k, "recipient-id", image_seed);
            xor_bitstring(&plain, &ks)
        }
        None => plain,
    }
}

/// Recover the plain id bitstring from a decoded payload bitstring.
pub fn decrypt_bits(decoded: &str, key: &str, image_seed: u64) -> String {
    let ks = crate::keying::keystream_bytes(key, "recipient-id", image_seed);
    xor_bitstring(decoded, &ks)
}

/// Encode a raw payload bitstring into the image via the learned layer.
pub fn embed_bits(
    img: image::DynamicImage,
    payload_bits: &str,
    dir: &Path,
    strength: f32,
) -> Result<image::DynamicImage> {
    let tm = load(dir)?;
    let out = tm
        .encode(payload_bits.to_string(), img, strength)
        .map_err(|e| anyhow!(e))?;
    // TrustMark returns Rgb32F; quantize to 8-bit RGB (PNG-friendly).
    Ok(image::DynamicImage::ImageRgb8(out.to_rgb8()))
}

/// Decode the learned watermark into a bitstring.
pub fn decode(img: image::DynamicImage, dir: &Path) -> Result<String> {
    let tm = load(dir)?;
    tm.decode(img).map_err(|e| anyhow!(e))
}
