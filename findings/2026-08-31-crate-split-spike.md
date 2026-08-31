# Spike: WASM Bundle and Build-Time Impact of Hypothetical Workspace Split

**Date:** 2026-08-31  
**Task:** `CTX-0017` — scope `sigil/` — no code change, measurements only  
**Branch:** `feat/wasm-in-memory-api` (`cf5f790`) + 1 uncommitted `carryctx` state  
**Toolchain:** `rustc 1.97.1`, `cargo 1.97.1`, `wasm-pack 0.15.0`, `cargo-bloat 0.12.1`, `wasm-opt 130`, `wasm32-unknown-unknown` installed

## Question

Would splitting the current single crate `sigil v0.2.0` into `sigil-core / sigil-dct / sigil-dwt` (and possibly `sigil-learned` / `sigil-c2pa`) reduce WASM bundle size or build times enough to justify the workspace overhead? Measurements are against `sigil` as consumed by `sigil-website/wasm-engine` (`sigil-wasm v0.1.0`, `crate-type = ["cdylib","rlib"]`, `opt-level=z`, `lto=true`, `codegen-units=1`, `panic=abort`).

---

## 1. Current single-crate baseline

### 1.1 Source layout

| Metric                         | Value                                                                                                                                                                             |
| ------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/*.rs` total lines         | **5,510** (`wc -l src/*.rs`)                                                                                                                                                      |
| `src/lib.rs` modules           | **18** `pub mod` (batch, c2pa cfg, c2pa_cli cfg, cli, dct, dwt, dwt_embed, embed, extract, geometry, info, keying, learned cfg, signal, spread_spectrum, strip, verify, wasm_api) |
| `src/wasm_api.rs`              | **124 lines**, 0 `#[cfg]` gates — `learned` rejected at runtime (`anyhow::bail!`), not compile-time                                                                               |
| Largest modules                | `dct.rs 1,096` · `dwt_embed.rs 716` · `embed.rs 610` · `verify.rs 514` · `cli.rs 453`                                                                                             |
| Hypothetical `sigil-core` core | `geometry 96` + `signal 139` + `keying 50` + `spread_spectrum 194` + `dwt 258` + `tiny-skia` + `vectomancy-raster/geometry` ≈ **~740 lines** before re-exports                    |

### 1.2 Dependency graph — `cargo tree --target wasm32-unknown-unknown`

```
sigil v0.2.0 — wasm32 (lib, default features)
  nodes (lines):          330
  native nodes:           340      (cargo tree without --target, same feature set)
  wasm nodes via wasm-engine: 362   (adds wasm-bindgen + console_error_panic_hook)
  unique crates (wasm):   160      (grep -oE '[a-z_-]+ v[0-9]' | sort -u)

depth 1 (wasm, sigil crate):
  anyhow, clap, digest, glob, hmac, image, serde, serde_json, sha2, tiny-skia,
  tracing, vectomancy-geometry, vectomancy-raster

depth 1 (native adds):
  tracing-subscriber                          ← target-gated correctly

depth 2+ notable transitivity:
  image v0.25.10 → exr, gif, png, qoi, ravif→rav1e, tiff, image-webp, moxcms, ...
  vectomancy-raster → image v0.25 (features = ["webp"]) + imageproc 0.26.2 + rayon + tracing
    imageproc → nalgebra 0.34.2 + approx + itertools + ab_glyph/ttf-parser + num-complex
  image (via vectomancy-raster with webp) unifies features so ravif/rav1e/exr/tiff are
    present even though sigil's own image dep is default-features=false,features=["png","jpeg"]
```

Full `cargo tree --target wasm32-unknown-unknown | head -n 100` is trimmed above; the authoritative counts are `330` (lib) / `362` (via `sigil-wasm`) lines.

### 1.3 Leakage audit (wasm32)

| Crate                                                                           | In `wasm32` graph?         | Expected?                                                                                             | Verdict                                                                                                                                                                                                                            |
| ------------------------------------------------------------------------------- | -------------------------- | ----------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tracing-subscriber 0.3.23`                                                     | **absent**                 | gated via `[target.'cfg(not(target_arch="wasm32"))'.dependencies]`                                    | ✅ correct                                                                                                                                                                                                                         |
| `clap 4.6.6` (`clap_builder`, `clap_derive`, `clap_lex`, `strsim`, `anstream`…) | **present** (depth 1)      | CLI-only; should be `cfg(not(wasm32))` or `optional`                                                  | **🔴 leakage** — links into every wasm build, dead-code-eliminated but not removed from compile graph                                                                                                                              |
| `glob 0.3.4`                                                                    | **present**                | `batch.rs` only; not used by `wasm_api`                                                               | **🔴 leakage** — same issue as clap                                                                                                                                                                                                |
| `rayon 1.12 + rayon-core 1.13 + crossbeam-*`                                    | **present**, 9 occurrences | via `vectomancy-raster` and `image` (`rav1e`, `av-scenechange`)                                       | ⚠️ benign on wasm — compiles to single-threaded shim (`rayon` detects `wasm32` and disables threads), but still pays compile time                                                                                                  |
| `imageproc 0.26.2` (+ `nalgebra`, `simba`, `num-complex`, `approx`)             | **present**, 1 occurrence  | via `vectomancy-raster` — used for skeleton/edge extraction (`verify::extract_geometry_from_image`)   | ⚠️ required for `wasm_api::verify_bytes` which derives geometry in-memory; `cargo bloat` shows `imageproc` itself is `891 B` after LTO (most monomorphised code eliminated), so runtime cost is near-zero but compile cost remains |
| `rav1e 0.8.1 / ravif 0.13.0 / exr / tiff / gif / qoi / png / webp`              | **present**                | transitive via `image` feature unification (`vectomancy-raster` enables `webp`, pulls full codec set) | **🟡 heavy** — dominates binary size (see §1.4); not needed for PNG/JPEG-only wasm path                                                                                                                                            |
| `trustmark / c2pa / directories / ureq / rcgen / x509-parser / time`            | **absent**                 | `optional` features, not enabled for wasm                                                             | ✅ correct                                                                                                                                                                                                                         |

**Headline:** `clap` + `glob` leak into wasm; `tracing-subscriber` is correctly gated; `rayon`/`imageproc` are present but effectively inert after LTO; image-codec bloat (`ravif`/`rav1e`) is the real size driver.

### 1.4 Binary size

#### Native `cargo bloat --release --crates` (bin `sigil`, `opt-level=3`, `lto=true`)

`cargo bloat --crates` only supports `bin`/`cdylib`/`dylib`; `sigil`'s `lib` is `rlib`-only so the bin is measured. Run `cargo bloat --release --crates -n 60` (warm, `Finished in 0.08s`):

```
 File  .text     Size Crate
14.1%  27.7%   1.7MiB ravif
 6.2%  12.1% 754.2KiB sigil
 5.8%  11.5% 713.7KiB image
 5.4%  10.7% 662.4KiB std
 4.8%   9.4% 585.1KiB rav1e
 2.3%   4.6% 285.9KiB exr
 2.2%   4.4% 273.0KiB clap_builder
 2.0%   4.0% 247.2KiB image_webp
 1.1%   2.2% 138.7KiB rayon_core
 1.1%   2.2% 137.7KiB tiff
 0.5%   1.0%  64.9KiB png
 0.3%   0.6%  39.8KiB tracing_subscriber
 0.3%   0.6%  38.0KiB gif
 0.3%   0.6%  35.7KiB vectomancy_raster
 0.3%   0.6%  34.9KiB weezl
 0.3%   0.6%  34.3KiB av_scenechange
 ... (53 more crates 205 KiB)
50.8% 100.0%   6.1MiB .text   file 11.9MiB
```

Notable after LTO: `imageproc 891 B`, `moxcms 1.0 KiB`, `tiny-skia 10.7 KiB`, `vectomancy-geometry 2.8 KiB`. The watermark code itself is small; image codecs dominate.

Release rlib: `target/release/libsigil.rlib 16 MiB`, `target/release/sigil 9.8 MiB` (stripped: `6.1 MiB` text).

#### WASM — `sigil-website/wasm-engine` via `wasm-pack build --target web`

Config: `sigil-wasm` depends on `sigil { default-features=false }`, `opt-level=z`, `lto=true`, `codegen-units=1`, `panic=abort`, `wasm-opt` enabled (binaryen `version 130`).

| Artifact                                                                                                              | Size                                       | Notes                                                               |
| --------------------------------------------------------------------------------------------------------------------- | ------------------------------------------ | ------------------------------------------------------------------- |
| `target/wasm32-unknown-unknown/release/sigil_wasm.wasm` (raw `cargo build --release --target wasm32-unknown-unknown`) | **5.1 MiB**                                | before `wasm-opt`                                                   |
| `pkg/sigil_wasm_bg.wasm` (after `wasm-opt -Oz`)                                                                       | **2.7 MiB**                                | `wasm-pack` default pipeline, `found wasm-opt at /usr/bin/wasm-opt` |
| `pkg/sigil_wasm_bg.wasm` (with `--no-opt`)                                                                            | **5.6 MiB**                                | raw without wasm-opt, confirms ~2× shrinkage from binaryen          |
| `pkg/sigil_wasm_bg.wasm` gzipped                                                                                      | **978 KiB** (`gzip -c \| wc -c = 977,686`) | what the browser actually downloads                                 |
| `pkg/sigil_wasm.js` glue                                                                                              | **12 KiB**                                 | `__wbg_*` + `wasm-bindgen` init                                     |
| `pkg/sigil_wasm_bg.wasm.d.ts`                                                                                         | **919 B**                                  | types                                                               |

No `twiggy`/`wasm-objdump` available in this image; codec breakdown inferred from native `cargo bloat` — wasm proportions are similar (std + image codecs dominate, `sigil` logic is <15%).

`wasm_api` feature gates: **none** at compile time. The 4 `#[wasm_bindgen]` shims in `sigil-website/wasm-engine/src/lib.rs` (`embed_image`, `verify_image`, `extract_id`, `version` + `start` panic hook) delegate to `sigil::wasm_api::{embed_bytes, verify_bytes, extract_bytes}`; `learned` is a runtime `bail!`, not a `cfg`.

---

## 2. Build times

All times `TIMEFORMAT="real %R user %U sys %S"` via `bash -c 'time …'`, measured on this host (`rustc 1.97.1`, warm `target/` reused unless noted "cold" = `cargo clean` first). `cargo check` is the CI-relevant metric.

| Command                                                                 | Cold (`cargo clean` first)                                                           | Warm (artifacts cached)                                                     | Incremental (one file touched, `src/dct.rs`) |
| ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------ | --------------------------------------------------------------------------- | -------------------------------------------- |
| `cargo check` (native, dev)                                             | **17.0 s** `real 17.018` `user 220.930` `sys 11.548`                                 | **0.10 s** `real 0.101`                                                     | **0.24 s** `real 0.263`                      |
| `cargo check --target wasm32-unknown-unknown --lib`                     | **10.9 s** `real 10.874` `user 75.484` `sys 10.090`                                  | **0.09 s** `real 0.093–0.096`                                               | **0.20 s** `real 0.226`                      |
| `cargo check --target wasm32-unknown-unknown` (incl. bin)               | fails — `tracing_subscriber` unresolved in `src/main.rs` (expected; bin is not wasm) | —                                                                           | —                                            |
| `cargo check --target wasm32-unknown-unknown` via `sigil-wasm`          | **11.8 s** `real 11.841` `user 78.315` `sys 10.128`                                  | **0.08–0.09 s**                                                             | —                                            |
| `cargo build --release` (native)                                        | **33.5 s** `real 33.492` `user 472.593` `sys 18.784`                                 | **34.3 s** warm still recompiles `rav1e`/`image` due to `opt-level=3`+`lto` | —                                            |
| `wasm-pack build --target web` (`sigil-wasm`, cold after `cargo clean`) | **19.1 s** wall (`Done in 19.14s`, `cargo` 16.82 s + `wasm-opt` 2.46 s)              | **2.46 s** `wasm-opt` only, cargo 0.06 s when warm                          | —                                            |

Observation: wasm `--lib` check is **faster** than native check (no `tracing-subscriber`, fewer native-only proc-macros). Incremental cost of touching one watermark module is ~0.2 s regardless of target — the crate is small enough that `cargo`'s crate-granularity is not a bottleneck.

---

## 3. Hypothetical split — `sigil-core / sigil-dct / sigil-dwt`

### 3.1 Postulated layout (one plausible cut; not implemented)

```
sigil-core          — geometry, signal, keying, spread_spectrum, tiny-skia glue,
                      vectomancy-raster/geometry, image (png/jpeg), tracing
                      (~740 lines + shared deps; everything wasm_api::embed_bytes needs
                       before it branches on mode)
sigil-dct           — dct.rs (1,096) + embed.rs/verify.rs/extract.rs DCT paths
                      depends on sigil-core; no new heavy deps
sigil-dwt           — dwt.rs (258) + dwt_embed.rs (716) + DWT paths in embed/verify/extract
                      depends on sigil-core
sigil (facade)      — re-exports core+dct+dwt, retains cli/batch/c2pa/learned/strip/info
                      bin `sigil` unchanged
```

Rejected alternative `sigil-core/sigil-dct/sigil-dwt` as fully independent crates with duplicated `[dependencies]` was not considered — all estimates below assume a **single Cargo workspace** with `workspace.dependencies` and `lto=true` + `codegen-units=1` at the workspace root, so large deps (`image`, `vectomancy-raster`, `tiny-skia`, `sha2/hmac`, `rayon`, `tracing`) are resolved once. CLI-only deps (`clap`, `glob`) would be moved to `[target.'cfg(not(target_arch="wasm32"))'.dependencies]` or `optional` and thus removed from the wasm graph entirely.

### 3.2 Estimated overhead of the split

| Cost                                            | Estimate                                                                                                                                                              | Basis                                                                                                             |
| ----------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Extra `Cargo.toml` + `src/lib.rs`               | 2 files, ~30 lines each                                                                                                                                               | `cargo new` boilerplate                                                                                           |
| Extra `version`/`publish` churn                 | 3× bump on every release if versions are locked together; `cargo-release` can batch but human cost remains                                                            | observed in `vectomancy` crates (geometry/transform/raster/svg)                                                   |
| `cargo tree` nodes                              | +2 workspace members, ~+10 lines of tree (the members themselves), **0 extra unique crates** when deduplicated                                                        | workspace.dependencies dedup                                                                                      |
| Compile graph nodes (wasm)                      | unchanged (362) minus `clap`/`glob` if gated ≈ **340**                                                                                                                | clap/glob removal saves ~22 nodes                                                                                 |
| `cargo check` cold                              | **no faster** — same total dep graph, one more `cargo` partition to schedule; expect **+0.2–0.5 s** from extra crate boundaries                                       | measured single-crate `cargo check --lib wasm` 10.9 s                                                             |
| `cargo check` warm incremental (touch `dct.rs`) | **marginally faster**: only `sigil-dct` + facade recheck, not `sigil-dwt` (~0.12–0.15 s vs 0.20–0.26 s today)                                                         | extrapolate from 0.22 s single-crate incremental                                                                  |
| `cargo build --release`                         | **same or slower** — LTO must cross crate boundaries (`lto=true` still works across workspace but `codegen-units=1` advantage is reduced)                             | rustc LTO docs                                                                                                    |
| WASM bundle (`wasm-opt` 2.7 MiB, gz 978 KiB)    | **no shrinkage**; possibly **+5–20 KiB** if LTO cannot inline across crates as aggressively; no shared-dep deduplication gain because there is nothing to deduplicate | native bloat shows `sigil` is only 12% of text; codecs dominate                                                   |
| API boundary tax                                | `GeometryFile`, `SignalMetrics`, `KeyMaterial`, `Spread` types must become `pub` with semver guarantees; internal `crate::` imports become `sigil_core::`             | breaks `cargo test` locality for unit tests currently in `dct.rs`/`dwt_embed.rs` (`super::*` → integration tests) |

### 3.3 Current vs hypothetical — summary table

| Dimension                                          | Current (single crate `sigil v0.2.0`)                 | Hypothetical (`sigil-core / sigil-dct / sigil-dwt` workspace)                                       | Delta                               |
| -------------------------------------------------- | ----------------------------------------------------- | --------------------------------------------------------------------------------------------------- | ----------------------------------- |
| Source                                             | 1 crate, 20 modules, 5,510 lines, 124-line `wasm_api` | 3–4 crates, ~5,600 lines incl. boilerplate, same `wasm_api` (now in `sigil` facade or `sigil-core`) | +2 `Cargo.toml`, +80 lines          |
| `cargo tree --target wasm32-unknown-unknown` lines | 330 (lib) / 362 via `sigil-wasm`                      | 332–340 (lib) / 364 via facade                                                                      | +2–10                               |
| Unique crates (wasm)                               | 160                                                   | 160 (deduped)                                                                                       | 0                                   |
| `clap` in wasm                                     | **present** (leakage)                                 | **absent** if `cfg(not(wasm32))`-gated as part of split                                             | fixite — achievable without a split |
| `tracing-subscriber` in wasm                       | absent (correct)                                      | absent                                                                                              | 0                                   |
| `rayon`/`imageproc` in wasm                        | present, inert after LTO (imageproc 891 B)            | present (still via `sigil-core` → `vectomancy-raster`)                                              | 0                                   |
| Native `cargo bloat` top crate                     | `ravif 1.7 MiB` (14.1%), file 11.9 MiB text 6.1 MiB   | identical — codecs not moved                                                                        | 0                                   |
| WASM `wasm-opt` size                               | **2.7 MiB** raw opt, **978 KiB gzipped**              | **2.70–2.72 MiB** opt, **980–985 KiB gz** (est.)                                                    | +0–20 KiB if LTO less effective     |
| `cargo check` cold wasm `--lib`                    | **10.9 s**                                            | **11.1–11.4 s** (est.)                                                                              | +0.2–0.5 s                          |
| `cargo check` warm incr (touch 1 module)           | **0.20–0.26 s**                                       | **0.12–0.15 s** for DCT-only change                                                                 | **−0.08–0.10 s** (only win)         |
| `cargo build --release`                            | 33.5 s cold                                           | 33–34 s                                                                                             | 0                                   |
| `wasm-pack build --target web` cold                | 19.1 s (16.82 + 2.46 wasm-opt)                        | 19–20 s                                                                                             | +0–1 s                              |
| `cargo test` wall (native)                         | 0.12 s (18 itests, 8 wasm_api tests)                  | ~0.15 s + extra `--workspace` flag                                                                  | +0.03 s                             |
| Maintenance                                        | 1 version, 1 publish                                  | 3 versions, 3 publishes, API boundary stabilization required before `CTX-0022`                      | **higher**                          |

---

## 4. What would actually move the needle (without a split)

The split does not address the real size/build drivers. Cheaper wins, in priority order:

1. **Gate CLI deps out of wasm** — move `clap` and `glob` (and any future `clap`-only transitive like `anstream`) to `[target.'cfg(not(target_arch="wasm32"))'.dependencies]` exactly as `tracing-subscriber` is today. Expected: `cargo tree --target wasm32-unknown-unknown` drops 20+ nodes, `wasm-opt` −50–100 KiB (clap_builder alone is 273 KiB in native bloat; after LTO less but still non-zero), `cargo check --target wasm32` −0.2–0.4 s. No workspace split required.

2. **Trim `image` feature unification** — `vectomancy-raster` enables `image/features=["webp"]` which, via feature unification, pulls `ravif`/`rav1e`/`exr`/`tiff` into every `sigil` consumer. For the wasm path only PNG+JPEG are used. Options: (a) make `vectomancy-raster`'s `image` dep default-off-features + feature-gated `webp`; (b) override in `sigil` with `image = { default-features=false, features=["png","jpeg"] }` and rely on resolver v2 to not unify across path deps (verify with `cargo tree --target wasm32-unknown-unknown -e features`). Native bloat suggests `ravif` (1.7 MiB) + `rav1e` (585 KiB) + `exr` (286 KiB) + `tiff` (138 KiB) + `image-webp` (247 KiB) together are **~2.9 MiB** of text — removing them would cut the native bin by ~25% and wasm `wasm-opt` 2.7 MiB by a comparable ~0.6–0.8 MiB (and gzipped 978 KiB by ~200–300 KiB). This dwarfs any split benefit.

3. **Keep `wasm_api` as 124-line facade** — it already isolates the wasm boundary (`embed_bytes` / `verify_bytes` / `extract_bytes` with fixed `GeometryParams { detail:60, min_path_len:5, chaikin_iters:3, color:false, stroke=0.010, placement=Skeleton }`). No per-mode crate needed.

4. **Do not introduce `sigil-dct`/`sigil-dwt` crates until `CTX-0018`/`CTX-0019`/`CTX-0022` decide the extraction API** — the `Carrier` trait and `sigil-core` extraction boundary (`research/media-credential`, `CTX-0019`) will dictate the real crate boundary. Premature `dct`/`dwt` crates would lock `GeometryFile`/`SignalMetrics`/`Spread` as public API before the framing/ECC/interleave layer (`CTX-0020`) exists.

---

## 5. Recommendation

**Do not split `sigil` into `sigil-core / sigil-dct / sigil-dwt` now.**

- The split **does not shrink the WASM bundle** (dominant cost is image codecs, not watermark logic; `sigil` itself is 12% of native `.text`, watermark modules are 2.5 k lines). Estimated wasm delta is zero to +20 KiB after LTO.
- The split **does not meaningfully speed builds** — cold `cargo check --target wasm32 --lib` is already 10.9 s and warm is 0.09 s; `wasm-pack` cold is 19 s. The only win is ~0.08–0.10 s faster incremental when touching a single watermark module (0.20 s → 0.12 s), not worth 3× versioning and a `GeometryFile` public-API lock before `CTX-0018` (carrier trait) and `CTX-0019` (extraction API spec).
- The leakage fix the split would incidentally achieve (removing `clap`/`glob` from wasm) is **one line each** via `target.'cfg(not(target_arch="wasm32"))'` — do that instead.
- The actionable size win is the **image codec trim** (potential −0.6–0.8 MiB wasm-opt, −200–300 KiB gzipped), not modularization.

**Proposed sequencing:**

- **Now (CTX-0017):** record this spike, take no code action.
- **CTX-0018 (single-crate trait enforcement):** keep `sigil` as one crate; enforce `Carrier` trait and module grouping internally (`sigil::dct`/`sigil::dwt` stay `pub(crate)` with `sigil::carrier` facade). Gate `clap`/`glob` out of wasm in the same PR — single `Cargo.toml` diff, big wasm-tree payoff.
- **CTX-0019 (extraction API spec):** define the `sigil-core` boundary on paper (what `GeometryFile`/`SignalMetrics` are public, what stays crate-private, how framing/ECC will plug in) without creating the crate yet.
- **CTX-0022 (extract `sigil-core`):** create **one** workspace member `sigil-core` (the trait + geometry + signal + keying + spread), keep `dct`/`dwt` as feature-gated modules inside `sigil-core` or `sigil` — do not create `sigil-dct`/`sigil-dwt` as separate crates until a concrete consumer (e.g., `sigild` or an alternate wasm bundle that needs only `dwt`) justifies it. Re-measure `wasm-opt` after the `image` codec trim before deciding.

---

## 6. How to reproduce

```bash
# toolchain
rustc --version; cargo --version; wasm-pack --version; cargo bloat --version; wasm-opt --version
rustup target list --installed | grep wasm32

# single-crate measurements
cargo tree --target wasm32-unknown-unknown | head -n 100
cargo tree --target wasm32-unknown-unknown | wc -l          # 330
cargo tree --target wasm32-unknown-unknown | grep -c rayon  # 9 (shim)
cargo tree --target wasm32-unknown-unknown | grep -c imageproc  # 1 (891 B after LTO)
cargo tree --target wasm32-unknown-unknown | grep -E "clap|glob|tracing-subscriber"
cargo bloat --release --crates -n 60                         # ravif 1.7MiB dominates
bash -c 'TIMEFORMAT="real %R user %U sys %S"; time cargo check --target wasm32-unknown-unknown --lib'
bash -c 'TIMEFORMAT="real %R user %U sys %S"; time cargo build --release'

# wasm bundle
wasm-pack build --target web --manifest-path sigil-website/wasm-engine/Cargo.toml
ls -lh sigil-website/wasm-engine/pkg/sigil_wasm_bg.wasm sigil-website/wasm-engine/target/wasm32-unknown-unknown/release/sigil_wasm.wasm
gzip -c sigil-website/wasm-engine/pkg/sigil_wasm_bg.wasm | wc -c   # 977686
cargo tree --target wasm32-unknown-unknown --manifest-path sigil-website/wasm-engine/Cargo.toml | head -n 60

# verification (must all pass)
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo check --target wasm32-unknown-unknown --lib
```

---

## 7. Raw outputs archived

- `cargo tree --target wasm32-unknown-unknown` (330 lines, wasm 160 unique crates) — §1.2
- `cargo bloat --release --crates` (6.1 MiB text, 11.9 MiB file, ravif 27.7%) — §1.4
- `cargo check` cold/warm timings (17.0 s native / 10.9 s wasm lib / 11.8 s wasm-engine) + incremental 0.20–0.26 s — §2
- `wasm-pack` cold 19.1 s (cargo 16.82 + wasm-opt 2.46), `wasm-opt` 5.1 MiB → 2.7 MiB, gz 978 KiB — §1.4
- `cargo test` 18 passed 2 ignored + 8 wasm_api passed — §6
