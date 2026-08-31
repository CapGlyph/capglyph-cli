# capglyph-core Extraction API — Crate Boundary Sketch

**Date:** 2026-08-31 (updated 2026-08-31 CTX-0022 → CTX-0040, renamed 2026-08-31 CTX-0039 Sigil → CapGlyph)
**Task:** CTX-0019 → CTX-0022 → CTX-0040
**Full spec:** [`capglyph-docs/research/media-credential/capglyph-core-api.md`](../../capglyph-docs/research/media-credential/capglyph-core-api.md)
**Status:** CTX-0040 — standalone `CapGlyph/capglyph-core` repo (canonical Rust Core, v0.1.0) extracted from `capglyph-cli/crates/capglyph-core`; `capglyph-cli` now depends via `path = "../capglyph-core"` (isolated monorepo)
**Issue:** [#13](https://github.com/CapGlyph/capglyph-cli/issues/13) (originally legacy Sigil repo #13, now CapGlyph/capglyph-cli, redirects)

This file is the **capglyph-repo-local sketch** of the shared `capglyph-core` boundary (formerly `sigil-core`).
The normative spec lives in `capglyph-docs` (formerly `sigil-docs`); this file exists so `cargo test` reviewers
and CI can verify the migration plan without crossing repos.

## Workspace after CTX-0040 (v0.1.0, formerly v0.2.0 Sigil; reset 2026-08-31 CTX-0044)

CTX-0022 embedded `crates/capglyph-core` inside `capglyph-cli` as a workspace member. CTX-0040 mechanically extracts it to a standalone canonical repo:

```
capglyph/
  capglyph-cli/
    Cargo.toml            # [dependencies] capglyph-core = { path = "../capglyph-core" }  (CTX-0040, was crates/capglyph-core)
    src/                  # binary crate (thin CLI wrappers → capglyph_core::Carrier via carrier.rs facade, alias sigil_core)
    docs/capglyph-core-api.md  # this file (formerly sigil-core-api.md)
  capglyph-core/          # standalone lib (canonical Rust Core, CapGlyph/capglyph-core, v0.1.0)
                          # sigil-core-api §3.3 feature gates, no clap/glob/c2pa/trustmark
                          # signal/keying/spread_spectrum/geometry/framing/ecc/interleave/registration + carrier trait + Placement
                          # (formerly crates/sigil-core → crates/capglyph-core)
    Cargo.toml
    src/{carrier,ecc,framing,geometry,interleave,keying,placement,registration,signal,spread_spectrum}.rs
    .github/workflows/ci.yml  # standalone ci: fmt/clippy/test + wasm-check (no vectomancy)
  capglyph-wasm/ # thin wasm bridge: capglyph-core wasm-safe subset (no secrets) — deferred to CTX-0023+ (formerly sigil-wasm)
  vectomancy -> /vectomancy/vectomancy  # symlink for raster/geometry path deps (capglyph-cli only)
```

Before CTX-0040:

```
capglyph-cli/
  Cargo.toml              # [workspace] members = ["crates/capglyph-core"]   (CTX-0022)
  crates/capglyph-core/   # embedded lib (deleted in CTX-0040, now standalone at ../capglyph-core)
```

## What moves to capglyph-core (formerly sigil-core)

- `signal.rs` (`SignalMetrics`) — pure `&[u8]` function, shared by CLI/verify/wasm
- `keying.rs` — HMAC PRF, split into `K_embed` (placement) / `K_mac` (framing tag) / `K_object` (pointer AEAD)
- `spread_spectrum.rs` — deprecated shim until `ecc` replaces repetition-8
- `geometry.rs` — `GeometryFile` (serde), cover vault + carrier lattice
- `carrier.rs` (`Carrier` trait + `Placement` + `AlphaCarrier`) — single dispatch point (CTX-0018), `DctCarrier`/`DwtCarrier` impls stay in `capglyph/src/carrier.rs` (legacy `sigil/src/carrier.rs`, facade) until `dct`/`dwt` move in follow-up
- `core.rs` grouping shim — replaced by `pub use capglyph_core::*` in `capglyph/src/lib.rs` + `capglyph/src/core.rs` re-exports (alias `sigil_core` retained)
- **CTX-0020:** `framing` (CBOR `version/length/type/flags` + HMAC) + `ecc` (BCH/RS + interleave + soft-bits LLR) + `interleave` — all moved in CTX-0022
- **CTX-0021:** `registration::Register` (`R = I_submitted^aligned - I_original` + correlation) + `CoverVault`/`HybridMatch` — moved in CTX-0022
- `dct.rs` + `dwt.rs` + `dwt_embed.rs` → `capglyph_core::carrier::{dct,dwt}` (alias `sigil_core`) — **deferred** (still in `capglyph` binary, uses `capglyph_core::Placement` via `carrier::to_core_placement` bridge)

## What stays in capglyph (binary, formerly sigil)

`cli.rs` (`clap` + `PlacementStrategy` parsing), `batch.rs` (`glob`), `c2pa.rs` (`c2pa` feature),
`learned.rs` (`trustmark` ONNX), `strip.rs`, `info.rs`, `embed.rs`/`verify.rs`/`extract.rs` thin dispatch,
`main.rs` (`tracing-subscriber`). None of these enter `capglyph-core` (formerly `sigil-core`) or the wasm graph.

## Public API sketch (minimal)

```rust
// capglyph-core/src/carrier/mod.rs (formerly sigil-core)
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
pub struct CapGlyphCore { pub carrier: CarrierKind, pub placement: Placement, pub framing: framing::Params, pub ecc: ecc::Profile } // formerly SigilCore

pub struct KeyMaterial { k_embed: [u8; 32], k_mac: [u8; 32], k_object: Option<[u8; 32]> }
pub trait Register { fn align(&self, original: &ImageBuffer<Rgb<u8>, Vec<u8>>, submitted: &ImageBuffer<Rgb<u8>, Vec<u8>>) -> anyhow::Result<AlignedImage>; }

#[cfg(feature = "wasm")] pub mod wasm_helpers { pub fn validate_frame(bytes: &[u8]) -> anyhow::Result<crate::framing::FrameHeader>; }
```

## Migration checklist (CTX-0022, no facade duplication) — DONE

## Extraction to standalone repo (CTX-0040) — DONE

CTX-0022:

- [x] Create `crates/capglyph-core/Cargo.toml` (`v0.1.0`, formerly `v0.2.0` Sigil, no `clap`/`glob`/`tracing-subscriber`/`c2pa`/`trustmark`, deps: `image` png/jpeg, `ciborium`, `serde_bytes`, `sha2`, `hmac`, `tracing`) (formerly `crates/sigil-core`)
- [x] Move `signal`/`keying`/`spread_spectrum`/`geometry`/`framing`/`ecc`/`interleave`/`registration`/`carrier` (trait+`Placement`+`AlphaCarrier`) verbatim into `crates/capglyph-core/src/` (formerly `crates/sigil-core`, `git mv` semantics, `cargo fmt` preserved)
- [x] `carrier` split: `capglyph_core::carrier::Carrier` (alias `sigil_core`) + `capglyph_core::placement::Placement` live in core; `capglyph/src/carrier.rs` (legacy `sigil/src/carrier.rs`) keeps `DctCarrier`/`DwtCarrier` impls as facade with `to_cli_placement`/`to_core_placement` bridge (no duplication of trait)
- [x] In `capglyph/Cargo.toml` (legacy `sigil/Cargo.toml`): add `[workspace] members = ["crates/capglyph-core"]`, `capglyph-core = { path = "crates/capglyph-core" }` (alias `sigil-core` retained via `capglyph_core`), `clap`/`glob` already gated via `cfg(not(wasm32))` (CTX-0030); kept `ciborium` etc for `capglyph`'s `dct`/`dwt` until they move
- [x] In `capglyph/src/lib.rs` (legacy `sigil/src/lib.rs`): replace `pub mod signal;` etc with `pub use capglyph_core::signal;` (alias `sigil_core` retained) — deleted moved files (`src/{signal,keying,spread_spectrum,geometry,framing,ecc,interleave,registration}.rs`), kept `src/core.rs` as thin `pub use capglyph_core::*` re-export and `src/carrier.rs` as `Carrier` impl facade
- [ ] Move `capglyph/src/wasm_api.rs` (legacy `sigil/src/wasm_api.rs`) → `capglyph-wasm/src/lib.rs` (formerly `sigil-wasm`) — deferred to CTX-0023+ (wasm_api stays in `capglyph` and re-uses `capglyph_core::signal` via `crate::signal`)
- [x] Version: `capglyph-core v0.1.0` (formerly `sigil-core v0.2.0` → reset 2026-08-31 CTX-0044) pinned, `capglyph v0.1.0` (formerly `sigil v0.2.0`) depends via path; semver bump to `0.2.0` deferred until `dct`/`dwt` move
- [x] CI gates: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo check --workspace --target wasm32-unknown-unknown`, `cargo tree --target wasm32-unknown-unknown -p capglyph-core` (legacy `-p sigil-core`) clean (85 nodes, no `clap`/`glob`/`tracing-subscriber`), `cargo tree -p capglyph --target wasm32` (legacy `-p sigil`) clean (195 nodes, `clap`/`glob` gated)

CTX-0040 (standalone canonical Rust Core):

- [x] Copy `capglyph-cli/crates/capglyph-core` (v0.1.0) → `CapGlyph/capglyph-core` repo as primary crate (`Cargo.toml` + `src/*.rs`), keep `v0.1.0` unchanged, no `clap`/`glob`/`c2pa`/`trustmark`
- [x] Populate `capglyph-core` repo: `Cargo.toml`, `src/*.rs` (identical to embedded), `LICENSE` (Apache-2.0), `README.md` (canonical-core docs + isolated monorepo path), `.gitignore`, `.github/workflows/ci.yml` (standalone fmt/clippy/test + wasm-check, no vectomancy sibling needed)
- [x] Update `capglyph-cli/Cargo.toml`: remove `[workspace] members = ["crates/capglyph-core"]`, change `capglyph-core = { path = "crates/capglyph-core" }` → `path = "../capglyph-core"` (isolated layout `../capglyph-core` sibling, same as `../vectomancy`; CI will checkout `CapGlyph/capglyph-core` at `capglyph-core` sibling)
- [x] Delete `capglyph-cli/crates/capglyph-core` directory (mechanical extraction, no duplicate) — `git rm -r crates/capglyph-core`
- [x] Update `capglyph-cli/.github/workflows/ci.yml` + `release.yml`: add `CapGlyph/capglyph-core` checkout at `capglyph-core` sibling, include `capglyph-core/Cargo.lock` in cache keys, update `prepare()` in AUR PKGBUILD to symlink `capglyph-core` sibling
- [x] Verify: `cargo test` in both repos, `cargo check --lib --target wasm32-unknown-unknown` in both, `cargo tree --target wasm32-unknown-unknown` clean (no `clap`/`glob`/`trustmark`/`c2pa` in either)
- [x] Dependency decision (documented in this file + `capglyph-core/README.md` + `capglyph-cli/Cargo.toml` comment): local dev uses `path = "../capglyph-core"` (isolated monorepo sibling); CI checks out sibling; crates.io publish will switch to `capglyph-core = "0.1"` version dep + `[patch.crates-io]` dev override or git dep `CapGlyph/capglyph-core` tag fallback — not yet published, so path dep remains canonical for now

## Verification gates

```bash
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo clippy --features learned,c2pa --all-targets -- -D warnings
cargo test
cargo test --features learned,c2pa
cargo check --lib --target wasm32-unknown-unknown -p capglyph-core   # after split (legacy -p sigil-core still works via alias)
cargo check --lib --target wasm32-unknown-unknown                # today: capglyph lib only (legacy sigil)
yamllint -d relaxed .
```

See full spec for `capglyphd` integration (legacy `sigild`, `Carrier::verify_original_assisted` + `framing::open` + `ecc::decode` + `registration::Register` + `DB::atomic_consume`) and for the `K_embed/K_mac` HKDF derivation.
