# sigil-core Extraction API — Crate Boundary Sketch

**Date:** 2026-08-31
**Task:** CTX-0019
**Full spec:** [`sigil-docs/research/media-credential/sigil-core-api.md`](../../sigil-docs/research/media-credential/sigil-core-api.md)
**Status:** Draft — API-only, no carrier constant change
**Issue:** [#13](https://github.com/Xuepoo/sigil/issues/13)

This file is the **sigil-repo-local sketch** of the shared `sigil-core` boundary.
The normative spec lives in `sigil-docs`; this file exists so `cargo test` reviewers
and CI can verify the migration plan without crossing repos.

## Workspace after CTX-0022 (v0.3)

```
sigil-core/    # new lib: carrier + signal/keying/spread_spectrum/geometry + framing/ecc/registration
sigil/         # binary crate (today's repo): thin CLI wrappers → sigil_core::Carrier
sigil-wasm/    # thin wasm bridge: sigil-core wasm-safe subset (no secrets)
```

## What moves to sigil-core

- `signal.rs` (`SignalMetrics`) — pure `&[u8]` function, shared by CLI/verify/wasm
- `keying.rs` — HMAC PRF, split into `K_embed` (placement) / `K_mac` (framing tag) / `K_object` (pointer AEAD)
- `spread_spectrum.rs` — deprecated shim until `ecc` replaces repetition-8
- `geometry.rs` — `GeometryFile` (serde), cover vault + carrier lattice
- `dct.rs` + `dwt.rs` + `dwt_embed.rs` → `sigil_core::carrier::{dct,dwt}`
- `carrier.rs` (`Carrier` trait + `DctCarrier`/`DwtCarrier`/`AlphaCarrier`) — single dispatch point (CTX-0018)
- `core.rs` grouping shim — deleted after move (replaced by `pub use sigil_core::*` in `sigil/src/lib.rs`)
- **New in CTX-0020:** `framing` (CBOR `version/length/type/flags` + HMAC/AEAD) + `ecc` (BCH/RS + interleave + soft-bits LLR)
- **New in CTX-0021:** `registration::Register` (`R = I_submitted^aligned - I_original` + correlation)

## What stays in sigil (binary)

`cli.rs` (`clap` + `PlacementStrategy` parsing), `batch.rs` (`glob`), `c2pa.rs` (`c2pa` feature),
`learned.rs` (`trustmark` ONNX), `strip.rs`, `info.rs`, `embed.rs`/`verify.rs`/`extract.rs` thin dispatch,
`main.rs` (`tracing-subscriber`). None of these enter `sigil-core` or the wasm graph.

## Public API sketch (minimal)

```rust
// sigil-core/src/carrier/mod.rs
pub enum Placement { Skeleton, Edge, Prng, Adaptive }
pub trait Carrier {
    const NAME: &'static str;
    type Metrics: core::fmt::Debug + serde::Serialize;
    fn embed(img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>, geometry: &crate::geometry::GeometryFile,
             payload: &[u8], keys: &KeyMaterial, placement: Placement) -> anyhow::Result<(u64, Vec<(u32,u32)>)>;
    fn verify(img: &ImageBuffer<Rgb<u8>, Vec<u8>>, geometry: &crate::geometry::GeometryFile,
              placement: Placement) -> anyhow::Result<Self::Metrics>;
    fn verify_original_assisted(original: &ImageBuffer<Rgb<u8>, Vec<u8>>,
              submitted_aligned: &ImageBuffer<Rgb<u8>, Vec<u8>>, keys: &KeyMaterial,
              placement: Placement) -> anyhow::Result<Self::Metrics>;
    fn verify_secret(img: &ImageBuffer<Rgb<u8>, Vec<u8>>, key: &str) -> f64;
    fn extract(img: &ImageBuffer<Rgb<u8>, Vec<u8>>) -> anyhow::Result<Vec<u8>>;
    fn metrics_is_present(metrics: &Self::Metrics, threshold: f64) -> bool;
    fn metrics_mean_signal(metrics: &Self::Metrics) -> f64;
}
pub struct DctCarrier; pub struct DwtCarrier; pub struct AlphaCarrier;
pub struct SigilCore { pub carrier: CarrierKind, pub placement: Placement, pub framing: framing::Params, pub ecc: ecc::Profile }

pub struct KeyMaterial { k_embed: [u8; 32], k_mac: [u8; 32], k_object: Option<[u8; 32]> }
pub trait Register { fn align(&self, original: &ImageBuffer<Rgb<u8>, Vec<u8>>, submitted: &ImageBuffer<Rgb<u8>, Vec<u8>>) -> anyhow::Result<AlignedImage>; }

#[cfg(feature = "wasm")] pub mod wasm_helpers { pub fn validate_frame(bytes: &[u8]) -> anyhow::Result<crate::framing::FrameHeader>; }
```

## Migration checklist (CTX-0022, no facade duplication)

- [ ] Create `sigil-core/Cargo.toml` (`wasm` feature, no `clap`/`glob`/`tracing-subscriber`/`c2pa`/`trustmark`)
- [ ] Move `signal/keying/spread_spectrum/geometry/dct/dwt/dwt_embed/carrier` verbatim into `sigil-core/src/` (keep paths for `git log --follow`)
- [ ] Add `sigil-core/src/framing` + `ecc` (CTX-0020) + `registration` (CTX-0021) — not in this PR
- [ ] In `sigil/Cargo.toml`: add `sigil-core = { path = "../sigil-core" }`, gate `clap`/`glob` with `cfg(not(wasm32))` (CTX-0030), remove moved deps
- [ ] In `sigil/src/lib.rs`: replace `pub mod signal;` etc with `pub use sigil_core::signal;` — delete moved files, delete `core.rs` shim
- [ ] Move `sigil/src/wasm_api.rs` → `sigil-wasm/src/lib.rs` (keep `embed_bytes/verify_bytes/extract_bytes`)
- [ ] Bump `sigil-core` + `sigil` to `0.3.0` with `sigil-core = "=0.3.0"` pin; `Carrier` is `#[non_exhaustive]`-aware
- [ ] CI gate: `cargo check --target wasm32-unknown-unknown -p sigil-core` + `cargo tree --target wasm32 | grep -v clap/glob` must be clean

## Verification gates

```bash
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo clippy --features learned,c2pa --all-targets -- -D warnings
cargo test
cargo test --features learned,c2pa
cargo check --lib --target wasm32-unknown-unknown -p sigil-core   # after split
cargo check --lib --target wasm32-unknown-unknown                # today: sigil lib only
yamllint -d relaxed .
```

See full spec for `sigild` integration (`Carrier::verify_original_assisted` + `framing::open` + `ecc::decode` + `registration::Register` + `DB::atomic_consume`) and for the `K_embed/K_mac` HKDF derivation.
