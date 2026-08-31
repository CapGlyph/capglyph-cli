# sigil-core Extraction API — Crate Boundary Sketch

**Date:** 2026-08-31 (updated 2026-08-31 CTX-0022)
**Task:** CTX-0019 → CTX-0022
**Full spec:** [`sigil-docs/research/media-credential/sigil-core-api.md`](../../sigil-docs/research/media-credential/sigil-core-api.md)
**Status:** Implemented (CTX-0022) — `crates/sigil-core` extracted, `sigil` re-exports, no facade duplication
**Issue:** [#13](https://github.com/Xuepoo/sigil/issues/13)

This file is the **sigil-repo-local sketch** of the shared `sigil-core` boundary.
The normative spec lives in `sigil-docs`; this file exists so `cargo test` reviewers
and CI can verify the migration plan without crossing repos.

## Workspace after CTX-0022 (v0.2.0)

```
sigil/
  Cargo.toml              # [workspace] members = ["crates/sigil-core"]
  crates/sigil-core/      # new lib: signal/keying/spread_spectrum/geometry/framing/ecc/interleave/registration + carrier trait + Placement
  src/                    # binary crate (thin CLI wrappers → sigil_core::Carrier via carrier.rs facade)
  docs/sigil-core-api.md  # this file
sigil-wasm/    # thin wasm bridge: sigil-core wasm-safe subset (no secrets) — deferred to CTX-0023+
```

## What moves to sigil-core

- `signal.rs` (`SignalMetrics`) — pure `&[u8]` function, shared by CLI/verify/wasm
- `keying.rs` — HMAC PRF, split into `K_embed` (placement) / `K_mac` (framing tag) / `K_object` (pointer AEAD)
- `spread_spectrum.rs` — deprecated shim until `ecc` replaces repetition-8
- `geometry.rs` — `GeometryFile` (serde), cover vault + carrier lattice
- `carrier.rs` (`Carrier` trait + `Placement` + `AlphaCarrier`) — single dispatch point (CTX-0018), `DctCarrier`/`DwtCarrier` impls stay in `sigil/src/carrier.rs` (facade) until `dct`/`dwt` move in follow-up
- `core.rs` grouping shim — replaced by `pub use sigil_core::*` in `sigil/src/lib.rs` + `sigil/src/core.rs` re-exports
- **CTX-0020:** `framing` (CBOR `version/length/type/flags` + HMAC) + `ecc` (BCH/RS + interleave + soft-bits LLR) + `interleave` — all moved in CTX-0022
- **CTX-0021:** `registration::Register` (`R = I_submitted^aligned - I_original` + correlation) + `CoverVault`/`HybridMatch` — moved in CTX-0022
- `dct.rs` + `dwt.rs` + `dwt_embed.rs` → `sigil_core::carrier::{dct,dwt}` — **deferred** (still in `sigil` binary, uses `sigil_core::Placement` via `carrier::to_core_placement` bridge)

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

## Migration checklist (CTX-0022, no facade duplication) — DONE

- [x] Create `crates/sigil-core/Cargo.toml` (`v0.2.0`, no `clap`/`glob`/`tracing-subscriber`/`c2pa`/`trustmark`, deps: `image` png/jpeg, `ciborium`, `serde_bytes`, `sha2`, `hmac`, `tracing`)
- [x] Move `signal`/`keying`/`spread_spectrum`/`geometry`/`framing`/`ecc`/`interleave`/`registration`/`carrier` (trait+`Placement`+`AlphaCarrier`) verbatim into `crates/sigil-core/src/` (`git mv` semantics, `cargo fmt` preserved)
- [x] `carrier` split: `sigil_core::carrier::Carrier` + `sigil_core::placement::Placement` live in core; `sigil/src/carrier.rs` keeps `DctCarrier`/`DwtCarrier` impls as facade with `to_cli_placement`/`to_core_placement` bridge (no duplication of trait)
- [x] In `sigil/Cargo.toml`: add `[workspace] members = ["crates/sigil-core"]`, `sigil-core = { path = "crates/sigil-core" }`, `clap`/`glob` already gated via `cfg(not(wasm32))` (CTX-0030); kept `ciborium` etc for `sigil`'s `dct`/`dwt` until they move
- [x] In `sigil/src/lib.rs`: replace `pub mod signal;` etc with `pub use sigil_core::signal;` — deleted moved files (`src/{signal,keying,spread_spectrum,geometry,framing,ecc,interleave,registration}.rs`), kept `src/core.rs` as thin `pub use sigil_core::*` re-export and `src/carrier.rs` as `Carrier` impl facade
- [ ] Move `sigil/src/wasm_api.rs` → `sigil-wasm/src/lib.rs` — deferred to CTX-0023+ (wasm_api stays in `sigil` and re-uses `sigil_core::signal` via `crate::signal`)
- [x] Version: `sigil-core v0.2.0` pinned, `sigil v0.2.0` depends via path; semver bump to `0.3.0` deferred until `dct`/`dwt` move
- [x] CI gates: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo check --workspace --target wasm32-unknown-unknown`, `cargo tree --target wasm32-unknown-unknown -p sigil-core` clean (85 nodes, no `clap`/`glob`/`tracing-subscriber`), `cargo tree -p sigil --target wasm32` clean (195 nodes, `clap`/`glob` gated)

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
