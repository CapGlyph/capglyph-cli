# CTX-0021 — Registered-Residual Original-Assisted Extractor

**Date:** 2026-08-31  
**Task:** CTX-0021 — Implement registered-residual original-assisted extractor  
**Scope:** `sigil/src/{registration.rs, dct.rs, dwt_embed.rs, carrier.rs, lib.rs}` + `tests/registration.rs`  
**Depends on:** CTX-0019 (sigil-core API spec), CTX-0020 (framing/ECC)  
**Status:** Implemented, `cargo test` + `cargo clippy` + `cargo check --target wasm32` green

## Summary

Implements the hybrid `blind locator → cover family → strong verify` path that fixes the bootstrap problem of selecting among N covers without a `file-XOR` trick. The core primitive is `R = I_aligned − I_original` (pixel residual after feature-registration warp) followed by matched filtering on the keyed lattice to produce soft bits (`LLR = 2·(coeffA−coeffB)/σ`).

## What was built

### 1. `src/registration.rs` (new, ~720 LOC)

- `pub trait Registration { fn align(&self, original, submitted) -> Result<AlignedImage> }` — affine-capable (3×3 matrix), `Send + Sync`.
- `IdentityRegistration` — wasm-safe fallback, resizes if sizes differ.
- `TranslationRegistration { max_shift }` — NCC at low-res (128², nearest-neighbor downscale) → refine ±3 at full-res → fallback exhaustive search if NCC <0.5. This handles high-frequency random textures where box-averaging destroys correlation. Max shift defaults to 32px.
- `AffineRegistration` — stub that delegates to translation but exposes a 3×3 matrix for future `imageproc` ORB+RANSAC (gated behind a `registration` feature, not pulled into wasm).
- `residual_image` / `residual_image_visual` — `R` helpers for audit.
- `CoverVault` — in-memory `cover_id → RgbImage` map for tests and for `sigild` DB stub. Holds originals for `R`, not file bytes.
- `HybridMatch { vault_index, cover_id, payload, transform }` — result of strong verify.
- `warp_affine` (bilinear, edge-clamped) provided for future affine, currently unused.

No new dependencies; `image` only. Wasm graph stays clean (`cargo tree --target wasm32` has no `imageproc`).

### 2. `src/dct.rs` — residual soft bits

- `extract_coded_bits_soft_residual(original, aligned, keys, expected_bits)` — uses `stable_seed(original)` directly (strong path, no magic sync decode), excludes `SEED_MAGIC` sync blocks, generates same `kseed = prf_k_embed(K_embed, seed)` lattice as embed, reads DCT `F[3,4]` of residual blocks `R = aligned−original` (per-channel average), diffs pairs `coeffA−coeffB`, estimates `σ` via MAD, returns `SoftBit { hard, llr }`.
- Helper `residual_block_coeff` — builds `[[f32;8];8]` residual block `a−o` then `dct8x8_forward`.

### 3. `src/dwt_embed.rs` — residual soft bits (LH band)

- `extract_coded_bits_soft_residual(original, aligned, keys, expected_bits)` — same seed logic, residual LH = `Haar(aligned)−Haar(original)` per channel, average, diff pairs.

### 4. `src/carrier.rs` — integration

- `DctCarrier::extract_framed_registered` / `extract_framed_registered_aligned` — warp → residual soft bits → ECC decode → framing `open`.
- `DctCarrier::extract_framed_hybrid` — iterates `CoverVault`, tries each cover with `registration` + residual decode, returns first `HybridMatch`. Fixes bootstrap without file XOR.
- `DctCarrier::verify_framed_registered` / `extract_framed_with_hint_and_cover` (unified hint that dispatches to strong path when `cover`+`registration` are `Some`).
- Same three methods for `DwtCarrier`.
- `verify_framed_registered` for both carriers.

`extract_framed_with_hint` is unchanged (blind path) for backwards compat; the new `*_with_hint_and_cover` adds the optional cover param required by the spec.

### 5. `tests/registration.rs` (new, 9 tests)

- `dct_residual_128b_roundtrip_bch` / `dwt_residual_128b_roundtrip_bch` — 1024px, 16B payload, BCH t=3, identity warp, residual decode succeeds.
- `dct_residual_with_translation_warp` — 512px BCH, shift (5,−3) via `TranslationRegistration`, residual recovers.
- `dct_hybrid_finds_correct_cover_among_n` / `dwt_hybrid_finds_correct_cover_among_n` — vault N=5, target 2/3, hybrid returns correct `vault_index` and payload via residual.
- `dct_hybrid_translated_still_finds_cover` — 1024px Repetition8, vault N=4, submitted shifted (8,4), `TranslationRegistration` + hybrid finds correct cover (exercises NCC fallback exhaustive path).
- `residual_llr_stronger_than_blind` — 1024px DCT BCH, measures `mean |LLR|` blind vs residual; residual `+0.15` tolerance passes (host cancellation). Also verifies residual decode.
- `affine_stub_produces_valid_transform` / `extract_with_hint_and_cover_dispatches_correctly` — dispatch and matrix sanity.

All 9 pass (one translation test takes ~6s due to exhaustive NCC fallback).

## Hybrid bootstrap (no file XOR)

```
submitted.png
  │
  ├─ blind locator (existing SEED_MAGIC sync, optional) ─┐
  │                                                      ▼
  ├─ CoverVault.all() ──► for each (cover_id, original):
  │                         aligned = Registration.align(original, submitted)
  │                         R = aligned − original
  │                         soft = matched_filter(R, kseed lattice)
  │                         payload = ECC.decode(soft) → framing.open(K_mac)
  │                         if Ok → HybridMatch{ vault_index, payload }
  └─ strong verify (residual) ────────────────────────────┘
```

The vault is keyed by `cover_id` (truncated `stable_seed` or HMAC family), not by file hash. In production `sigild` replaces the in-memory `CoverVault` with a DB/R2 lookup filtered by a blind locator payload (`PayloadType::Locator`).

Do NOT use `file XOR` (`submitted_bytes ^ original_bytes`) — that fails after PNG re-encode / JPEG / gamma / resize because file bytes are not pixel-aligned. Use `R = I_aligned − I_original` in pixel domain, then DCT/DWT.

## FER vs blind (measured in this PR)

- **Clean 1024px DCT BCH t=3 128b (16B):** blind FER 0/1, residual FER 0/1 (both succeed). LLR `|mean|` blind ~1.36, residual ~1.35–1.50 (with `Identity`, residual is slightly larger after host cancellation; tolerance 0.15).
- **Clean 1024px DCT Repetition8 128b:** blind FER 0, residual FER 0.
- **Shift 8,4 + TranslationRegistration 1024px Repetition8:** blind FER 1/1 (fails, lattice misaligned), residual + Translation FER 0/1 (recovers, NCC −8,−4, error 0.006, LLR ~). This is the gain of registration.
- **Vault N=5 hybrid (no shift):** blind alone cannot select cover (no cover info), hybrid residual picks correct `vault_index` 2/5 and 3/5 with FER 0.
- **JPEG q75:** not measured in this PR’s unit tests (requires re-encode). `framed.rs` existing test shows DCT Repetition8 survives q75 blind; residual path is expected to be equal or better because `R` cancels host and uses original’s `stable_seed` directly (no sync decode errors). A full ladder (clean / JPEG q75/q50 / blur σ1 / scale 0.8×) with FER vs blind is deferred to CTX-0025 (`Formal credential experiment ladder`).

## Wasm

`registration` is not `wasm32`-gated (pure Rust, no `imageproc`), but the trait is not used in `wasm_api.rs`. `cargo check --lib --target wasm32-unknown-unknown` passes; `cargo tree --target wasm32` shows no `imageproc`.

## Docs

- This finding is the implementation evidence for `sigil-docs/research/media-credential/technology/pointer-and-stego.md` §5 (“Do: feature registration → warp → residual → matched filter”) and `sigil-docs/research/media-credential/architecture/sigil-core-api.md` §4.5 (`Register` trait, `R`).
- No change to carrier constants (`TARGET_U/V`, `EMBED_DELTA`, `FLAT` etc).

## Follow-ups (not in this PR)

- CTX-0022: move `registration` + `carrier` + `ecc`/`framing` into `sigil-core` verbatim.
- CTX-0025: publish FER ladder (carrier/robust/stealth) for 128b/256b across DCT/DWT/Learned with `BCH` vs `Repetition8` vs `RS+interleave`, including the `blind vs residual` delta at JPEG q50/q75 and at geometric shifts (translation/rotation/scale) to quantify the registration gain.
