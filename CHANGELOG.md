# Changelog

All notable changes to CapGlyph (formerly Sigil) are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Changed

- Docs rename Sigil → CapGlyph (CTX-0039): `README.md`, `README.zh-CN.md`, `docs/mvp-spec.md`, `docs/product-roadmap.md`, `docs/roadmap.md`, `docs/sigil-core-api.md` → `docs/capglyph-core-api.md`, `.github/workflows/*`, `nfpm.yaml` — URLs updated to `https://github.com/CapGlyph/capglyph-cli` (legacy Sigil location now redirects), binary `sigil` → `capglyph` (alias `sigil` retained), env `SIGIL_*` → `CAPGLYPH_*` (fallback `SIGIL_*` retained), brew `xuepoo/tap` → `CapGlyph/tap`, scoop `Xuepoo/scoop-bucket` → `CapGlyph/scoop-bucket`, `com.sigil.watermark` → `com.capglyph.watermark` (legacy still read).

### Added

- CapGlyph org repos (CTX-0036): created `CapGlyph/capglyph-core`, `CapGlyph/capglyph-spec`, `CapGlyph/capglyph-test-vectors` (public) — verified via `gh repo list`/`gh repo view`; org now has 5 repos (`capglyph-cli` public, `capglyph-docs` private, plus 3 new). See `docs/capglyph-org-repos.md`.

## [0.2.0] - 2026-08-16

### Added

- C2PA content credentials: `capglyph c2pa init/sign/verify` (legacy `sigil c2pa`) — self-signed ES256
  certificate generation (rcgen), signing with pure-Rust crypto (no OpenSSL),
  verification with JSON report and exit codes (0 = valid, 1 = invalid,
  2 = unsigned)
- Dual-layer provenance: `embed --c2pa` / `verify --c2pa` sign and verify a
  C2PA manifest alongside the pixel watermark, cross-referencing mode,
  recipient ID, and keyed flag via the `com.capglyph.watermark` assertion
  (legacy `com.sigil.watermark` still recognized)
- Truthful provenance: `c2pa.created` digital source type parameterized via
  `--source-type` (`capture | algorithmic | composite | trained`), defaulting
  to `digitalCapture` to avoid false AI-origin attestation
- Optional `c2pa` cargo feature (`rust_native_crypto` + `file_io`); enabled in
  all release binaries, no system OpenSSL dependency

### Fixed

- `prng_block_list` infinite loop when the requested block count exceeded the
  image's 8×8 block count (e.g. `embed --mode dct --recipient-id` on images
  smaller than 512 blocks). The count is now capped; small images degrade
  gracefully instead of hanging.

## [0.1.0] - 2026-08-15

### Added

- Four watermark modes: `alpha` (sparse alpha-channel signal), `dct`
  (8×8 DCT coefficient modulation), `dwt` (Haar LH-band modulation),
  `learned` (TrustMark ONNX, optional `learned` feature)
- Three-layer security architecture: public presence detection, recipient-ID
  tracing (geometry-free self-sync extraction), HMAC keyed secret layer
  (survives collusion, blocks forgery)
- Keyed learned-mode payload: ID XOR-encrypted with HMAC keystream — ID
  privacy without the key, attribution proof with it
- `capglyph batch` (legacy `sigil batch`) bulk embedding/stripping with glob patterns and JPEG output
- `capglyph info` signal statistics without pass/fail threshold
- `capglyph extract` geometry-free recipient-ID recovery (DCT/DWT/learned)
- `capglyph fetch-models` downloads TrustMark ONNX models (proxy-aware)
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
