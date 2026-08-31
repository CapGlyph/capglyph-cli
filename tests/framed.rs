use image::{ImageBuffer, Rgb};
use sigil::carrier::{DctCarrier, DwtCarrier};
use sigil::ecc::Profile;
use sigil::framing::PayloadType;
use sigil::geometry::{AnalysisParams, GeometryFile, PathEntry};
use sigil::keying::KeyMaterial;

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

fn make_image(w: u32, h: u32) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    ImageBuffer::from_fn(w, h, |x, y| {
        let v = ((x * 7 + y * 13 + x * y / 3) % 251) as u8;
        Rgb([v, (v as u32 + 60) as u8 % 255, (v as u32 + 120) as u8 % 255])
    })
}

fn keys() -> KeyMaterial {
    KeyMaterial::from_keys([0x11u8; 32], [0x22u8; 32])
}

#[test]
fn dct_framed_128b_roundtrip_bch() {
    let (w, h) = (1024, 1024);
    let mut img = make_image(w, h);
    let geo = make_geometry(w, h);
    let payload: Vec<u8> = (0..16).collect(); // 128b
    let keys = keys();
    let profile = Profile::Bch { t: 3 };
    let (n, _) = DctCarrier::embed_framed(
        &mut img,
        &geo,
        &payload,
        &keys,
        &Default::default(),
        profile,
        PayloadType::Credential,
    )
    .unwrap();
    assert!(n > 0);
    let out = DctCarrier::extract_framed(&img, &keys, profile).unwrap();
    assert_eq!(out, payload, "DCT BCH roundtrip failed");
}

#[test]
fn dct_framed_128b_roundtrip_repetition() {
    let (w, h) = (1024, 1024);
    let mut img = make_image(w, h);
    let geo = make_geometry(w, h);
    let payload: Vec<u8> = (0..16).collect();
    let keys = keys();
    let profile = Profile::Repetition8;
    let (n, _) = DctCarrier::embed_framed(
        &mut img,
        &geo,
        &payload,
        &keys,
        &Default::default(),
        profile,
        PayloadType::Credential,
    )
    .unwrap();
    assert!(n > 0);
    let out = DctCarrier::extract_framed(&img, &keys, profile).unwrap();
    assert_eq!(out, payload);
}

#[test]
fn dwt_framed_128b_roundtrip_bch() {
    let (w, h) = (1024, 1024);
    let mut img = make_image(w, h);
    let geo = make_geometry(w, h);
    let payload: Vec<u8> = (0..16).map(|x| x + 0xA0).collect();
    let keys = keys();
    let profile = Profile::Bch { t: 3 };
    let (n, _) = DwtCarrier::embed_framed(
        &mut img,
        &geo,
        &payload,
        &keys,
        &Default::default(),
        profile,
        PayloadType::Credential,
    )
    .unwrap();
    assert!(n > 0);
    let out = DwtCarrier::extract_framed(&img, &keys, profile).unwrap();
    assert_eq!(out, payload);
}

#[test]
fn dct_framed_fer_jpeg_q75() {
    let (w, h) = (1024, 1024);
    let mut img = make_image(w, h);
    let geo = make_geometry(w, h);
    let payload: Vec<u8> = vec![0xAB; 16];
    let keys = keys();
    // Repetition is most robust for JPEG (soft LLR majority); BCH t=3 also works at q50 but fails at q75 with 1.6% BER
    let profile = Profile::Repetition8;
    DctCarrier::embed_framed(
        &mut img,
        &geo,
        &payload,
        &keys,
        &Default::default(),
        profile,
        PayloadType::Credential,
    )
    .unwrap();
    // JPEG q75 roundtrip via image crate
    let mut buf = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 75);
    enc.encode_image(&image::DynamicImage::ImageRgb8(img.clone()))
        .unwrap();
    let jpeg_img = image::load_from_memory(&buf).unwrap().to_rgb8();
    let result = DctCarrier::extract_framed(&jpeg_img, &keys, profile);
    match result {
        Ok(out) => assert_eq!(out, payload, "FER failure at q75"),
        Err(e) => panic!("FER decode failed at q75: {}", e),
    }
}
