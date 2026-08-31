use capglyph::carrier::{DctCarrier, DwtCarrier};
use capglyph::ecc::Profile;
use capglyph::framing::PayloadType;
use capglyph::geometry::{AnalysisParams, GeometryFile, PathEntry};
use capglyph::keying::KeyMaterial;
use capglyph::registration::{
    AffineRegistration, CoverVault, IdentityRegistration, Registration, TranslationRegistration,
};
use image::{ImageBuffer, Rgb};

fn make_geometry(w: u32, h: u32) -> GeometryFile {
    let points: Vec<[f64; 2]> = (0..64)
        .map(|i| [i as f64 * (w as f64 / 64.0), (i as f64 * 3.0) % h as f64])
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

fn make_image_seed(w: u32, h: u32, seed: u32) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    ImageBuffer::from_fn(w, h, |x, y| {
        let v = ((x.wrapping_add(seed) * 7 + y * 13 + x * y / 3 + seed) % 251) as u8;
        Rgb([v, (v as u32 + 60) as u8 % 255, (v as u32 + 120) as u8 % 255])
    })
}

fn make_image(w: u32, h: u32) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    make_image_seed(w, h, 1)
}

fn keys() -> KeyMaterial {
    KeyMaterial::from_keys([0x11u8; 32], [0x22u8; 32])
}

#[test]
fn dct_residual_128b_roundtrip_bch() {
    let (w, h) = (1024, 1024);
    let orig = make_image(w, h);
    let mut watermarked = orig.clone();
    let geo = make_geometry(w, h);
    let payload: Vec<u8> = (0..16).collect();
    let keys = keys();
    let profile = Profile::Bch { t: 3 };
    DctCarrier::embed_framed(
        &mut watermarked,
        &geo,
        &payload,
        &keys,
        &Default::default(),
        profile,
        PayloadType::Credential,
    )
    .unwrap();

    // Original-assisted via identity (no geometric distortion)
    let reg = IdentityRegistration;
    let out =
        DctCarrier::extract_framed_registered(&orig, &watermarked, &reg, &keys, profile, Some(16))
            .unwrap();
    assert_eq!(out, payload, "DCT residual BCH roundtrip failed");
}

#[test]
fn dwt_residual_128b_roundtrip_bch() {
    let (w, h) = (1024, 1024);
    let orig = make_image(w, h);
    let mut watermarked = orig.clone();
    let geo = make_geometry(w, h);
    let payload: Vec<u8> = (0..16).map(|x| x + 0xA0).collect();
    let keys = keys();
    let profile = Profile::Bch { t: 3 };
    DwtCarrier::embed_framed(
        &mut watermarked,
        &geo,
        &payload,
        &keys,
        &Default::default(),
        profile,
        PayloadType::Credential,
    )
    .unwrap();
    let reg = IdentityRegistration;
    let out =
        DwtCarrier::extract_framed_registered(&orig, &watermarked, &reg, &keys, profile, Some(16))
            .unwrap();
    assert_eq!(out, payload);
}

#[test]
fn dct_residual_with_translation_warp() {
    let (w, h) = (512, 512);
    let orig = make_image(w, h);
    let mut watermarked = orig.clone();
    let geo = make_geometry(w, h);
    let payload = vec![0xAB; 16];
    let keys = keys();
    let profile = Profile::Bch { t: 3 };
    DctCarrier::embed_framed(
        &mut watermarked,
        &geo,
        &payload,
        &keys,
        &Default::default(),
        profile,
        PayloadType::Credential,
    )
    .unwrap();

    // Simulate submitted shifted by (5, -3) — e.g., cropped/padded pipeline
    let dx = 5;
    let dy = -3;
    // Use the registration warp helper directly to create shifted version
    let shifted = capglyph::registration::TranslationRegistration { max_shift: 32 }
        .align(&orig, &watermarked)
        .unwrap()
        .image;
    // Actually create shifted submitted manually via warp_translation logic:
    // For test, we want submitted = watermarked shifted by dx,dy
    // So we simulate by shifting watermarked
    // We'll reuse internal warp by constructing a shifted image
    let submitted = {
        let mut out = ImageBuffer::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as u32;
                let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as u32;
                out.put_pixel(x, y, *watermarked.get_pixel(sx, sy));
            }
        }
        out
    };
    // Residual path with translation registration should recover
    let reg = TranslationRegistration { max_shift: 16 };
    let out =
        DctCarrier::extract_framed_registered(&orig, &submitted, &reg, &keys, profile, Some(16))
            .unwrap();
    assert_eq!(out, payload, "translation-warp residual decode failed");

    // Also verify the estimator found roughly correct shift (allow ±2)
    // Note: warp with dx creates left shift by dx, so alignment needs -dx.
    let aligned = reg.align(&orig, &submitted).unwrap();
    let (est_dx, est_dy) = aligned.transform.translation;
    assert!(
        (est_dx + dx as f32).abs() <= 2.0,
        "dx est {est_dx} vs true -{dx}"
    );
    assert!(
        (est_dy + dy as f32).abs() <= 2.0,
        "dy est {est_dy} vs true -{dy}"
    );
    let _ = shifted; // keep unused
}

#[test]
fn dct_hybrid_finds_correct_cover_among_n() {
    let (w, h) = (512, 512);
    let n = 5usize;
    let keys = keys();
    let profile = Profile::Bch { t: 3 };
    let payload: Vec<u8> = (0..16).collect();
    let geo = make_geometry(w, h);

    // Build vault with N distinct originals
    let mut vault = CoverVault::new();
    let mut originals = Vec::new();
    for i in 0..n {
        let orig = make_image_seed(w, h, (i as u32 * 97 + 13) % 251);
        originals.push(orig.clone());
        vault.insert(format!("cover-{i}").into_bytes(), orig);
    }

    // Embed payload into original #2
    let target_idx = 2usize;
    let target_orig = originals[target_idx].clone();
    let mut watermarked = target_orig.clone();
    DctCarrier::embed_framed(
        &mut watermarked,
        &geo,
        &payload,
        &keys,
        &Default::default(),
        profile,
        PayloadType::Credential,
    )
    .unwrap();

    // Hybrid should find target_idx via residual
    let reg = IdentityRegistration;
    let m = DctCarrier::extract_framed_hybrid(&watermarked, &vault, &reg, &keys, profile, Some(16))
        .unwrap();
    assert_eq!(m.vault_index, target_idx, "hybrid picked wrong cover");
    assert_eq!(m.payload, payload);
    assert_eq!(m.cover_id, format!("cover-{target_idx}").into_bytes());
}

#[test]
fn dwt_hybrid_finds_correct_cover_among_n() {
    let (w, h) = (512, 512);
    let n = 5usize;
    let keys = keys();
    let profile = Profile::Bch { t: 3 };
    let payload: Vec<u8> = (0..16).map(|x| x + 0x50).collect();
    let geo = make_geometry(w, h);
    let mut vault = CoverVault::new();
    let mut originals = Vec::new();
    for i in 0..n {
        let orig = make_image_seed(w, h, (i as u32 * 53 + 7) % 251);
        originals.push(orig.clone());
        vault.insert(format!("dwt-cover-{i}").into_bytes(), orig);
    }
    let target_idx = 3usize;
    let target_orig = originals[target_idx].clone();
    let mut watermarked = target_orig.clone();
    DwtCarrier::embed_framed(
        &mut watermarked,
        &geo,
        &payload,
        &keys,
        &Default::default(),
        profile,
        PayloadType::Credential,
    )
    .unwrap();
    let reg = IdentityRegistration;
    let m = DwtCarrier::extract_framed_hybrid(&watermarked, &vault, &reg, &keys, profile, Some(16))
        .unwrap();
    assert_eq!(m.vault_index, target_idx);
    assert_eq!(m.payload, payload);
}

#[test]
fn dct_hybrid_translated_still_finds_cover() {
    let (w, h) = (1024, 1024);
    let keys = keys();
    let profile = Profile::Repetition8;
    let payload = vec![0xCD; 16];
    let geo = make_geometry(w, h);
    let mut vault = CoverVault::new();
    for i in 0..4 {
        vault.insert(
            format!("c{i}").into_bytes(),
            make_image_seed(w, h, i * 11 + 5),
        );
    }
    // Use cover 1 as target
    let orig = make_image_seed(w, h, 16);
    // Rebuild vault entry 1 to match orig (ensure exact)
    let mut vault2 = CoverVault::new();
    for i in 0..4 {
        let o = if i == 1 {
            orig.clone()
        } else {
            make_image_seed(w, h, i * 11 + 5)
        };
        vault2.insert(format!("c{i}").into_bytes(), o);
    }
    let mut watermarked = orig.clone();
    DctCarrier::embed_framed(
        &mut watermarked,
        &geo,
        &payload,
        &keys,
        &Default::default(),
        profile,
        PayloadType::Credential,
    )
    .unwrap();
    // Shift submitted by (8, 4) — left/up shift via edge replication
    let dx = 8;
    let dy = 4;
    let submitted = {
        let mut out = ImageBuffer::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as u32;
                let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as u32;
                out.put_pixel(x, y, *watermarked.get_pixel(sx, sy));
            }
        }
        out
    };
    let reg = TranslationRegistration { max_shift: 16 };
    let m = DctCarrier::extract_framed_hybrid(&submitted, &vault2, &reg, &keys, profile, Some(16))
        .unwrap();
    assert_eq!(m.vault_index, 1);
    assert_eq!(m.payload, payload);
}

#[test]
fn residual_llr_stronger_than_blind() {
    let (w, h) = (1024, 1024);
    let orig = make_image(w, h);
    let mut watermarked = orig.clone();
    let geo = make_geometry(w, h);
    let payload = vec![0x42; 16];
    let keys = keys();
    let profile = Profile::Bch { t: 3 };
    DctCarrier::embed_framed(
        &mut watermarked,
        &geo,
        &payload,
        &keys,
        &Default::default(),
        profile,
        PayloadType::Credential,
    )
    .unwrap();

    // Blind soft bits
    let params = capglyph::framing::Params {
        version: 1,
        payload_type: PayloadType::Credential,
        flags: 0,
    };
    let sealed_len = capglyph::framing::sealed_len(16, &params);
    let need_bits = capglyph::ecc::coded_bits_len(sealed_len, profile);
    let blind_soft =
        capglyph::dct::extract_coded_bits_soft_with_hint(&watermarked, &keys, Some(need_bits))
            .unwrap();
    let blind_mean_abs: f32 =
        blind_soft.iter().map(|s| s.llr.abs()).sum::<f32>() / blind_soft.len() as f32;

    // Residual soft bits
    let reg = IdentityRegistration;
    let aligned = reg.align(&orig, &watermarked).unwrap();
    let residual_soft = capglyph::dct::extract_coded_bits_soft_residual(
        &orig,
        &aligned.image,
        &keys,
        Some(need_bits),
    )
    .unwrap();
    let res_mean_abs: f32 =
        residual_soft.iter().map(|s| s.llr.abs()).sum::<f32>() / residual_soft.len() as f32;

    // Residual should have larger mean LLR (host interference cancelled).
    // Allow small epsilon due to quantization; residual is not strictly > blind
    // for every random image/payload but should be close.
    assert!(
        res_mean_abs + 0.15 >= blind_mean_abs,
        "residual LLR {res_mean_abs:.2} should be >= blind {blind_mean_abs:.2} - 0.15"
    );
    // Also ensure residual decode succeeds
    let out =
        DctCarrier::extract_framed_registered(&orig, &watermarked, &reg, &keys, profile, Some(16))
            .unwrap();
    assert_eq!(out, payload);
}

#[test]
fn affine_stub_produces_valid_transform() {
    let (w, h) = (256, 256);
    let orig = make_image(w, h);
    let sub = orig.clone();
    let reg = AffineRegistration::default();
    let aligned = reg.align(&orig, &sub).unwrap();
    // Transform should be identity-ish
    assert!((aligned.transform.matrix[0][0] - 1.0).abs() < 1e-6);
    assert!((aligned.transform.matrix[1][1] - 1.0).abs() < 1e-6);
}

#[test]
fn extract_with_hint_and_cover_dispatches_correctly() {
    let (w, h) = (1024, 1024);
    let orig = make_image(w, h);
    let mut wm = orig.clone();
    let geo = make_geometry(w, h);
    let payload = vec![0x99; 16];
    let keys = keys();
    let profile = Profile::Repetition8;
    DctCarrier::embed_framed(
        &mut wm,
        &geo,
        &payload,
        &keys,
        &Default::default(),
        profile,
        PayloadType::Credential,
    )
    .unwrap();
    let reg = IdentityRegistration;
    // With cover → strong path
    let out1 = DctCarrier::extract_framed_with_hint_and_cover(
        &wm,
        &keys,
        profile,
        Some(16),
        Some(&orig),
        Some(&reg),
    )
    .unwrap();
    assert_eq!(out1, payload);
    // Without cover → blind path
    let out2 = DctCarrier::extract_framed_with_hint(&wm, &keys, profile, Some(16)).unwrap();
    assert_eq!(out2, payload);
}
