# Changelog

All notable changes to CapGlyph (formerly Sigil) are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.0] - 2026-08-31 — Initial CapGlyph release (formerly Sigil v0.2.0)

> **Note:** CapGlyph starts at **0.1.0** for a fresh org. This is an intentional
> semver _downgrade_ from Sigil `0.2.0`; the codebase is Sigil `0.2.0` renamed.
> Users migrating from `sigil v0.2.0` should treat `capglyph v0.1.0` as the
> successor (no data-format break; `sigil` binary/env/assertion aliases retained).

### Added

- Extracted `capglyph-core` crate (`crates/capglyph-core v0.1.0`, formerly `sigil-core v0.2.0`):
  pure codec primitives (signal/keying/spread_spectrum/geometry/framing/ecc/
  interleave/registration + `Carrier` trait + `Placement`) shared with future
  `capglyph-wasm` and `capglyphd`; `capglyph` re-exports via `pub use capglyph_core::*`
  (compat alias `sigil_core` retained).
- Isolated workspace layout (`capglyph-cli` + `vectomancy` siblings; path deps
  `vectomancy-raster`/`vectomancy-geometry`, no `vectomancy` facade; see `docs/capglyph-core-api.md`).
- WASM gating: `clap`/`glob`/`tracing-subscriber` under `cfg(not(wasm32))`
  (`cargo tree --target wasm32-unknown-unknown` clean), `image` codecs gated to
  `png/jpeg` (CTX-0031), `wasm_api.rs` in-memory API (CTX-0007..0012).
- Org repos `CapGlyph/capglyph-core`, `CapGlyph/capglyph-spec`,
  `CapGlyph/capglyph-test-vectors` (public) — see `docs/capglyph-org-repos.md` (CTX-0036).
- Docs rename Sigil → CapGlyph (CTX-0039): `README.md`, `README.zh-CN.md`,
  `docs/mvp-spec.md`, `docs/product-roadmap.md`, `docs/roadmap.md`,
  `docs/sigil-core-api.md` → `docs/capglyph-core-api.md`, `.github/workflows/*`,
  `nfpm.yaml` — URLs `https://github.com/CapGlyph/capglyph-cli`, binary `sigil`
  → `capglyph` (alias `sigil` retained), env `SIGIL_*` → `CAPGLYPH_*`
  (fallback `SIGIL_*` retained), `com.sigil.watermark` → `com.capglyph.watermark`.

### Changed

- Version reset `0.2.0` → `0.1.0` for both `capglyph` and `capglyph-core`
  (new-org fresh start; formerly `sigil v0.2.0`).

### Inherited from Sigil v0.2.0 / v0.1.0

The functional changes below were shipped under Sigil version numbers and are
carried forward unchanged in `capglyph v0.1.0`:

## [0.2.0] - 2026-08-16 (Sigil, now capglyph v0.1.0)

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

## [0.1.0] - 2026-08-15 (Sigil, now capglyph v0.1.0)

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
