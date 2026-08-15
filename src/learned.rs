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
        let mut resp = ureq::get(&url)
            .call()
            .with_context(|| format!("Failed to download {url}"))?;
        let data = resp
            .body_mut()
            .read_to_vec()
            .with_context(|| format!("Failed to read body of {url}"))?;
        std::fs::write(&tmp, &data).with_context(|| format!("Failed to write {:?}", tmp))?;
        std::fs::rename(&tmp, &target)
            .with_context(|| format!("Failed to move {:?} into place", tmp))?;
        tracing::info!("downloaded model {name}");
    }
    Ok(())
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

/// Encode the recipient id into the image via the learned layer.
pub fn embed(
    img: image::DynamicImage,
    recipient_id: &str,
    dir: &Path,
    strength: f32,
) -> Result<image::DynamicImage> {
    let tm = load(dir)?;
    let bits = id_to_bitstring(recipient_id);
    let out = tm.encode(bits, img, strength).map_err(|e| anyhow!(e))?;
    // TrustMark returns Rgb32F; quantize to 8-bit RGB (PNG-friendly).
    Ok(image::DynamicImage::ImageRgb8(out.to_rgb8()))
}

/// Decode the learned watermark into a bitstring.
pub fn decode(img: image::DynamicImage, dir: &Path) -> Result<String> {
    let tm = load(dir)?;
    tm.decode(img).map_err(|e| anyhow!(e))
}

/// Verify: decode and compare bit accuracy against the expected id.
/// Returns (bit_accuracy 0..=1, decoded_bitstring, expected_bitstring).
pub fn verify(
    img: image::DynamicImage,
    recipient_id: &str,
    dir: &Path,
) -> Result<(f64, String, String)> {
    let decoded = decode(img, dir)?;
    let expected = id_to_bitstring(recipient_id);
    let n = decoded.len().min(expected.len());
    let matches = decoded
        .as_bytes()
        .iter()
        .zip(expected.as_bytes().iter())
        .take(n)
        .filter(|(a, b)| a == b)
        .count();
    let acc = if n == 0 {
        0.0
    } else {
        matches as f64 / n as f64
    };
    Ok((acc, decoded, expected))
}
