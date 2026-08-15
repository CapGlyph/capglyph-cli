# Changelog

All notable changes to Sigil are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Four watermark modes: `alpha` (sparse alpha-channel signal), `dct`
  (8×8 DCT coefficient modulation), `dwt` (Haar LH-band modulation),
  `learned` (TrustMark ONNX, optional `learned` feature)
- Three-layer security architecture: public presence detection, recipient-ID
  tracing (geometry-free self-sync extraction), HMAC keyed secret layer
  (survives collusion, blocks forgery)
- Keyed learned-mode payload: ID XOR-encrypted with HMAC keystream — ID
  privacy without the key, attribution proof with it
- `sigil batch` bulk embedding/stripping with glob patterns and JPEG output
- `sigil info` signal statistics without pass/fail threshold
- `sigil extract` geometry-free recipient-ID recovery (DCT/DWT/learned)
- `sigil fetch-models` downloads TrustMark ONNX models (proxy-aware)
- `--recipient-id` per-recipient tracing, `--key` secret attribution
- Solid-color PRNG fallback (DCT/DWT embed on images without geometry)
- Alpha channel preservation on RGBA inputs; white compositing in `strip`
- Flat-region adaptive DWT strength (PSNR 34.7 → 45.4 dB on portraits)
- Spread-spectrum bit encoding with redundancy 8

### Verified

- Attack matrix across 4 modes: JPEG q30–q75, blur σ1–2, scale 0.5–1.5×,
  collusion median, known-cover diff, img2img regeneration
- VLM invisibility (gemini-3.5-flash-lite) and human visibility thresholds
- Zero false positives on clean images (mean-signal detection)
- Full findings in `../vectomancy-docs/findings/` (Q-series)
