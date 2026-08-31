# Review: CTX-0018 carrier trait + core grouping (CTX-0028)

**Reviewer:** opencode (reviewer subagent, CTX-0028)  
**Date:** 2026-08-31  
**Task:** CTX-0028 — Code Review of CTX-0018  
**Artifact reviewed:** `sigil` commit `d0f04e2` — `refactor(core): enforce carrier trait and core module group (CTX-0018)`  
**Branch context:** `feat/wasm-in-memory-api` @ `d0f04e2` (parent `f9f20e4` + ancestors `cf5f790` (CTX-0015 protocol v2) … `b8e0ad7`), base `main` @ `e953f1b`  
**Scope inspected:** `src/lib.rs`, `src/core.rs` (new), `src/carrier.rs` (new), `src/embed.rs` (dispatch via `Carrier`), `tests/c2pa_tests.rs` fix — plus transitive `Cargo.toml`/`Cargo.lock` unchanged, `verify.rs`/`wasm_api.rs`/`batch.rs` *not* changed by this commit (see §2.3)

---

## 1. Verification summary

### 1.1 Build & lint — reproduced on `d0f04e2` worktree

| Check | Command | Result | Verdict |
|-------|---------|--------|---------|
| `cargo fmt --check` | `cargo fmt --check --manifest-path Cargo.toml` (on `d0f04e2` with `/tmp/vectomancy` symlink for path deps) | no diff | ✅ pass |
| `cargo clippy -- -D warnings` (native, default) | `cargo clippy --manifest-path … --all-targets -- -D warnings` | 0 warnings | ✅ pass |
| `cargo clippy --features learned,c2pa` | same + `--features learned,c2pa` | 0 warnings (after 43s compile) | ✅ pass |
| `cargo test` (default) | `cargo test --manifest-path …` | `19 lib` + `18 integration` (2 ignored) + `8 wasm_api` + `0 c2pa` — **45 passed**, 0 failed | ✅ pass |
| `cargo check --lib --target wasm32-unknown-unknown` | `cargo check --lib --target wasm32-unknown-unknown` | `Finished` | ✅ pass |
| `cargo check --target wasm32-unknown-unknown` (incl. bin) | same without `--lib` | fails `tracing_subscriber` unresolved in `src/main.rs` — **expected**, because `main.rs` is bin-only and not gated for wasm | ⚠️ correctly handled by `--lib` gate; see §2.2 |
| `cargo tree --target wasm32-unknown-unknown` | `cargo tree --target wasm32-unknown-unknown \| wc -l` | **330** lines (lib), **362** via `sigil-website/wasm-engine` — identical to spike baseline | ✅ intact |

> Repro was hampered by `feat/wasm-in-memory-api` relative path `../vectomancy` — `/tmp` worktree required `ln -s /mnt/data/Workspace/Projects/vectomancy/vectomancy /tmp/vectomancy` or running from original checkout. Not a code defect, but a worktree portability note (see §2.5).

### 1.2 Behaviour preservation

- `carrier.rs` impls (`DctCarrier`, `DwtCarrier`) are **pure delegation** to existing `crate::dct`, `crate::dwt_embed`, `crate::extract` primitives — no new logic. `AlphaCarrier` is helper-only, no `Carrier` impl (documented as intentional, `Rgba` vs `Rgb` signature mismatch).
- `embed.rs` dispatch: `DctCarrier::embed` (5 args) and `DwtCarrier::embed_with_strength` (6 args incl. `dwt_strength`) mirror previous direct calls — verified by diff `embed.rs:14` + `253→` and `273→`. `dwt_strength` is threaded from `EmbedArgs::dwt_strength` (introduced in `cf5f790`, not this commit) — so this commit's *isolated* diff adds the wiring, the field itself predates it on this branch. On `main` without `cf5f790`, cherry-picking `d0f04e2` alone conflicts — branch pollution (see §2.5).
- `core.rs` re-exports are `pub use crate::geometry` etc — `crate::geometry` remains `pub mod geometry` in `lib.rs`, so downstream `use sigil::geometry::GeometryFile` and `use sigil::core::geometry::GeometryFile` resolve to **same type**. Verified by `cargo test` passing and by `grep` showing `verify.rs`/`info.rs` still use `crate::geometry` while `embed.rs` uses `crate::core::geometry` — both compile.
- `tests/c2pa_tests.rs` fix adds missing `VerifyArgs` fields (`protocol_version`, `min_alpha_pixels`) — pre-existing `clippy --features learned,c2pa` failure on `main`, correctly patched.

### 1.3 Dispatch correctness (DCT/DWT)

| Carrier | `embed` | `embed_with_strength` | `verify` | `verify_secret` | `extract` | `metrics_*` |
|---------|---------|------------------------|----------|-----------------|-----------|-------------|
| **DctCarrier** | ✅ delegates to `crate::dct::embed` with `placement` | default impl forwards to `embed` (DCT ignores strength) — correct | ✅ `crate::dct::verify` | ✅ `verify_secret` | ✅ `extract_from_dct` with `w,h` | `is_present` → `DctSignalMetrics::is_present(threshold as f32)`, `mean_signal` → `signal_strength` — mirrors `verify.rs` threshold `8.0` |
| **DwtCarrier** | ✅ delegates to `crate::dwt_embed::embed` | ✅ `embed_with_strength` with positive check `ensure!(strength>0.0)` | ⚠️ ignores `_placement` (explicitly noted) — see §2.1 | ✅ `verify_secret` | ✅ `extract_from_dwt` | `mean >= threshold \|\| (detection_rate>=0.8 && mean>=2.0)` — correctly mirrors `verify.rs` v1 logic, but v2 path in `dwt_embed::verify_v2` is **not** exposed via trait (see §2.3) |

### 1.4 Public API & WASM gate

- `lib.rs` adds `pub mod carrier; pub mod core;` — **additive**, no breaking change. `wasm_api.rs` unchanged, still compiles (`cargo check --lib --target wasm32` passes).
- `core.rs` docstring says `crate::core` will become `sigil-core` extraction point — accurate. The `pub use crate::carrier` inside `core.rs` makes `sigil::core::carrier::Carrier` an alias of `sigil::carrier::Carrier` — future `pub use sigil_core::carrier::Carrier` replacement is mechanical as claimed.
- WASM leakage: `clap` 4.6.6 + `glob` 0.3.4 still in `wasm32` tree (330 nodes) — **not fixed by this commit**. `tracing-subscriber` correctly gated via `[target.'cfg(not wasm32)'.dependencies]` — unchanged. This is the same defect flagged in CTX-0027 §2.2; still present at `d0f04e2`.

---

## 2. Defects and observations

### 2.1 🟡 DEFECT — `DwtCarrier::verify` silently ignores `PlacementStrategy`

- **Location:** `src/carrier.rs:137-147`
  ```rust
  fn verify(img: &…, geometry: &…, _placement: &PlacementStrategy) -> Result<Self::Metrics> {
      // DWT placement is currently geometry-only; placement is ignored …
      crate::dwt_embed::verify(img, geometry)
  }
  ```
- **Impact:** Trait contract claims `verify(img, geometry, placement)` but DWT ignores the caller's `Edge`/`Prng` choice. `embed.rs` *does* pass `placement` through `DwtCarrier::embed_with_strength`, but `verify` will always use skeleton path. This is inconsistent with `DctCarrier::verify` which honors placement, and with `verify.rs` which now (post-`cf5f790`) correctly dispatches `Edge`/`Prng`/`Skeleton` via `edge_blocks_with_budget` / `prng_blocks_with_budget`. A caller using `Carrier::verify` for DWT will get different block-set semantics than a caller going directly to `dwt_embed::verify` — which is identical today because `dwt_embed::verify` itself ignores placement, but the divergence will bite when DWT gains placement-aware verify (e.g. for `verify_v2`).
- **Recommendation:** Either (a) make `DwtCarrier::verify` error when `placement != Skeleton` with a clear `bail!("DWT verify placement {:?} not yet implemented")`, or (b) thread placement through to a new `dwt_embed::verify_with_placement` (mirroring `dct::verify` changes in `cf5f790`). At minimum, document the limitation in trait-level `verify` docstring as `/// # Note: DWT ignores placement; Edge/Prng are no-ops`.
- **Follow-up:** New task `CTX-00XX` — align DWT carrier placement handling (or explicitly reject non-Skeleton).

### 2.2 🟡 DEFECT — `carrier::Carrier` over-groups `metrics_*` helpers that duplicate `verify.rs`/`wasm_api.rs` thresholds

- **Location:** `src/carrier.rs:48-59` — `metrics_is_present` + `metrics_mean_signal`; `DwtCarrier::metrics_is_present` re-implements `mean >= threshold || (detection_rate >=0.8 && mean>=2.0)` which also lives in `verify.rs:341-347` and `wasm_api.rs:verify`.
- **Impact:** Three copies of the DWT decision rule (carrier, verify, wasm_api) will drift. The DCT side similarly duplicates `is_present(threshold as f32)` — less risky but still 2 sites.
- **Recommendation:** Keep `verify` returning `Metrics` but move the decision rule to **one** place: either `Metrics::is_present_at(threshold)` impl on the metrics structs themselves, or a free function `crate::verify::is_present(metrics, threshold, protocol)` that both `carrier` and `verify.rs` call. Carrier trait can then drop `metrics_is_present` / `metrics_mean_signal` or delegate to `Metrics` inherent methods.
- **Follow-up:** Task — de-duplicate DWT/DCT presence predicates.

### 2.3 🟡 OBSERVATION — Carrier trait adoption is partial (embed-only)

- **Location:** `src/verify.rs:1-5` still `use crate::geometry::GeometryFile;` and calls `crate::dct::verify` / `crate::dwt_embed::verify` directly; `src/info.rs`, `src/wasm_api.rs` (124 lines, no `Carrier` import) likewise bypass trait. Only `src/embed.rs` dispatches via `Carrier`.
- **Impact:** The trait's stated goal "prepares mechanical extraction of `sigil-core`" is **not yet realized** — `sigil-core` would need to move `dct.rs`/`dwt_embed.rs`/`extract.rs` wholesale, but verify/wasm paths would still depend on crate-root primitives. This is acknowledged in commit message ("No new crates yet"), but reviewers should expect a second PR that migrates `verify.rs` + `wasm_api.rs` to `Carrier::verify`/`extract` or explicitly decides not to.
- **Follow-up:** Task CTX-0019 (spec) should decide whether `verify`/`extract` also go through `Carrier` or remain free functions; if yes, file CTX-00XX to complete dispatch.

### 2.4 🟡 OBSERVATION — `AlphaCarrier` does not implement `Carrier`

- **Location:** `src/carrier.rs:170-211` — `pub struct AlphaCarrier;` with `verify_rgba`/`verify_rgba_v2` helpers, no `impl Carrier for AlphaCarrier`.
- **Rationale is documented:** alpha needs `RgbaImage` vs trait's `Rgb` signature. That's valid, but it means `sigil-core` enumeration of `all carriers` will still need a special case. Options: (a) split trait into `RgbCarrier` + `RgbaCarrier`, or (b) keep `AlphaCarrier` as helper as now and accept the asymmetry. No code change required — just ensure CTX-0019 spec calls this out so `sigil-core` extraction doesn't try to unify them forcefully.
- **Follow-up:** Note in CTX-0019 spec; no code task unless spec decides otherwise.

### 2.5 🔴 DEFECT — Branch pollution: `d0f04e2` (CTX-0018) depends on unmerged CTX-0015 (protocol v2) — not cherry-pickable to `main`

- **Location:** `git log --oneline main..d0f04e2` shows `d0f04e2` on top of `f9f20e4` (`docs findings`) on top of `cf5f790` (`feat: add protocol v2 detector …`) which adds `EmbedArgs::dwt_strength`, `VerifyArgs::protocol_version`/`min_alpha_pixels`, `EDGE_THRESHOLD`, `verify_v2`, etc. `d0f04e2`'s `embed.rs` adds `args.dwt_strength` threading and `Carrier::embed_with_strength` usage that **assumes** those fields exist.
- **Impact:** `git cherry-pick d0f04e2` onto `main` (e953f1b) **conflicts** in `src/embed.rs` and cannot be tested in isolation. `cargo check --lib --target wasm32` on `main` + cherry-pick fails without also pulling `cf5f790...`. This means CTX-0018 cannot land before CTX-0015 (in_progress) without a rebase that extracts the `dwt_strength` wiring into CTX-0015 or splits `d0f04e2` into a pure trait-only commit. Current `main` is `e953f1b` which is *behind* `cf5f790` — review had to use a full `feat/wasm-in-memory-api` checkout at `d0f04e2` with path-workaround symlink to reproduce.
- **Recommendation:** Before merging CTX-0018, either (a) merge CTX-0015 first, or (b) rebase `d0f04e2` onto `main` by extracting the `dwt_strength` param from `embed.rs` in this commit (keep `DwtCarrier::embed_with_strength` but call it with `DWT_EMBED_STRENGTH` constant on `main` branch, and let `cf5f790` later add the CLI flag). This was not done — the commit message's verification claim (`cargo check --lib --target wasm32` at `d0f04e2`) is true only on the polluted branch, not on `main`.
- **Follow-up:** Task — rebase/clean CTX-0018 for `main` or merge CTX-0015 before it; do not merge `feat/wasm-in-memory-api` as a squash without separating concerns.

### 2.6 🟡 OBSERVATION — `core.rs` uses `pub use` not `pub mod` — rustdoc/module-path nuance

- **Location:** `src/core.rs:21-27`
  ```rust
  pub use crate::geometry;
  pub use crate::keying;
  // …
  pub use crate::carrier;
  ```
- **Impact:** `crate::core::geometry` is **not** a submodule but a re-export of the same `crate::geometry` module. `cargo doc` will show `core::geometry` as a re-export, not a child module — which is correct for `pub use`, but the docstring says "Re-export the four core modules as `crate::core::geometry` etc." That's accurate, but the phrasing "grouped for future `sigil-core` extraction" might mislead readers expecting `core::geometry` to be a real inline module. The extraction plan "move `core.rs` + `carrier.rs` into new crate and keep `pub use sigil_core::…`" is indeed mechanical, but it will require changing `pub use` to `pub use sigil_core::…` or `pub mod`.
- **Recommendation:** No code change now; just ensure CTX-0019/0022 docs distinguish `pub use` alias vs `pub mod` and that `cargo doc --open` renders as expected.

### 2.7 🟡 OBSERVATION — `embed_to_image` now has 7 args, suppressed via `#[allow(clippy::too_many_arguments)]`

- **Location:** `src/embed.rs:229` — `#[allow(clippy::too_many_arguments)] pub(crate) fn embed_to_image(img, mode, geometry, stroke, recipient_id, key, placement, dwt_strength)`
- **Impact:** Not a defect — the `allow` is correctly placed and the function is `pub(crate)` with 4 call sites (`embed`, `batch`, `wasm_api`, `extract_and_build_geometry`). Future `sigil-core` extraction might want a `EmbedConfig` struct to group `recipient_id`/`key`/`placement`/`dwt_strength`/`stroke`, but that's out of scope for this refactor.
- **Follow-up:** Optional — introduce `CarrierEmbedConfig` when CTX-0022 moves `Carrier` to `sigil-core`.

### 2.8 🔴 DEFECT — `clap`/`glob` still unconditional in `Cargo.toml` — CTX-0018 missed wasm leakage fix

- **Location:** `Cargo.toml:22` (`clap`) and `48` (`glob`) are `[dependencies]`, not `[target.'cfg(not wasm32)'.dependencies]` like `tracing-subscriber`. `cargo tree --target wasm32-unknown-unknown` still shows `clap 4.6.6` + `glob 0.3.4` (330 nodes); expected ~308 without them.
- **Impact:** Same as CTX-0027 §2.2 — ~22 extra wasm nodes, extra codegen. Findings `2026-08-31-crate-split-spike.md` §4.1 explicitly says "Gate `clap`/`glob` out of wasm in the same PR" for CTX-0018 — not done.
- **Follow-up:** Already filed as `CTX-0030 Gate clap and glob out of wasm` (ready). This review re-affirms that CTX-0018 tip `d0f04e2` still leaks; CTX-0030 should be merged before or alongside CTX-0018 rebase.

### 2.9 🟡 OBSERVATION — `carrier.rs` imports `crate::geometry` not `crate::core::geometry`

- **Location:** `src/carrier.rs:15` — `use crate::geometry::GeometryFile;` while `embed.rs` uses `crate::core::geometry`.
- **Impact:** Minor inconsistency — both resolve to same type today, but `core::geometry` is the intended future path. Changing `carrier.rs` to `use crate::core::geometry::GeometryFile;` would make the "mechanical extraction" claim stronger (all new code uses `core::`). Not a bug.
- **Follow-up:** One-line follow-up or include in CTX-0030/0022 cleanup.

---

## 3. Trait design assessment — minimal vs over-abstraction

**Verdict: Minimal enough, and correctly staged.**

- The trait is **thin**: 5 required methods (`embed`, `verify`, `verify_secret`, `extract`, `metrics_*`×2) plus 1 default (`embed_with_strength`). Each impl is 15–20 lines of delegation. No generics, no dyn, no async, no GATs — statical dispatch via `DctCarrier::embed` etc. This is the right weight for a pre-extraction seam: call sites in `embed.rs` are ready to become `sigil_core::Carrier` without changing signatures.
- Alternatives considered and rejected (in review): a single `enum Carrier { Dct, Dwt, Alpha }` with `match` would centralize dispatch but hide placement/strength differences; a `dyn Carrier` with `Box<dyn Carrier>` would add vtable cost for no benefit. The chosen `struct + trait` with associated `Metrics` is the idiomatic Rust pattern for carrier-specific return types.
- `embed_with_strength` default impl is pragmatic: DCT ignores strength, DWT uses it. The alternative — two separate traits `Carrier` / `StrengthCarrier` — would be overkill.
- `AlphaCarrier` not implementing `Carrier` is a **correct** asymmetry — forcing `Rgb<u8>` vs `Rgba<u8>` into one signature would leak `ImageBuffer` generics or require `enum Image` — that *would* be over-abstraction. Keeping it as helper with `verify_rgba` is the minimal choice.
- The only over-grouping is `metrics_is_present`/`metrics_mean_signal` (see §2.2) — those could be inherent methods on the metrics structs instead of trait methods, but having them on the trait makes `verify.rs`/`wasm_api.rs` call sites uniform (`Carrier::metrics_is_present(&m, thr)`). Acceptable for Phase 1; de-duplicate later.

**Does it help `sigil-core` extraction?**

Yes — `core.rs` + `carrier.rs` together are **self-contained**: `core.rs` re-exports the four primitives (`geometry`, `signal`, `keying`, `spread_spectrum`) that have **zero** dependency on `clap`/`glob`/`tracing`/`learned`/`c2pa`; `carrier.rs` depends only on `crate::geometry`, `crate::dct`, `crate::dwt_embed`, `crate::extract`. Moving those 6 files (`core.rs`, `carrier.rs`, `geometry.rs`, `signal.rs`, `keying.rs`, `spread_spectrum.rs`, `dwt.rs`) into `sigil-core` and keeping `pub use sigil_core::{geometry,…}` in the facade is mechanical as claimed. The only non-mechanical part is `dct.rs`/`dwt_embed.rs`/`extract.rs` which also depend on `crate::cli::PlacementStrategy` — that enum will need to move to `sigil-core` or be duplicated.

---

## 4. Backwards compatibility

- `lib.rs` keeps `pub mod geometry; pub mod signal; pub mod keying; pub mod spread_spectrum;` — so `sigil::geometry::GeometryFile`, `sigil::signal::SignalMetrics`, etc. **still compile**. Verified by `grep` that `verify.rs` and `info.rs` still `use crate::geometry`.
- `core.rs` additionally exposes `sigil::core::geometry` etc — additive, no break. `sigil::core::carrier::Carrier` is alias of `sigil::carrier::Carrier` — same `TypeId`.
- Public API surface grows by 2 modules (`carrier`, `core`) — semver minor, not patch, but `v0.2.0` → `v0.2.x` still plausible as internal refactor; no `pub` fields removed.
- No `Cargo.toml` feature or version bump — acceptable for internal refactor branch; before release, bump or document in `CHANGELOG.md`.

---

## 5. Recommendations & follow-up tasks

| # | Task | Priority | Description |
|---|------|----------|-------------|
| 1 | **`CTX-0030` (existing)** | **P0** | Gate `clap` + `glob` out of wasm (`[target.'cfg(not wasm32)'.dependencies]`) — re-verify `cargo tree wasm32` drops to ~308, `check --lib --target wasm32` still passes. |
| 2 | **`CTX-0031` (existing)** | P1 | Trim `image` codec unification via `vectomancy-raster` feature gate (`webp` off by default) — saves 0.6–0.8 MiB wasm-opt, dominates split benefit. |
| 3 | **CTX-00XX — Rebase CTX-0018 for `main`** | **P0** | Rebase `d0f04e2` onto `main` without `cf5f790` pollution, or merge CTX-0015 (`protocol v2`) first. Ensure `feat/wasm-in-memory-api` can be merged as two separate PRs (`protocol v2` + `carrier trait`) or as one squashed PR with clean separation. |
| 4 | **CTX-00XX — Align DWT carrier placement** | P1 | Fix `DwtCarrier::verify` ignoring `_placement` — either error on non-`Skeleton` or implement placement-aware DWT verify (incl. `verify_v2` path). Unblocks CTX-0015 placement comparisons for DWT. |
| 5 | **CTX-00XX — De-duplicate presence predicates** | P2 | Move `metrics_is_present` / `metrics_mean_signal` logic to `DctSignalMetrics::is_present` + `DwtSignalMetrics::is_present` inherent methods or a single `verify::is_present` helper; update `carrier.rs`, `verify.rs`, `wasm_api.rs` to call one site. |
| 6 | **CTX-0019 spec** | P1 | Spec `sigil-core` extraction API must decide: (a) does `verify`/`extract` also go through `Carrier`? (b) is `AlphaCarrier` a `Carrier` impl or helper? (c) where does `PlacementStrategy` live? Record in `sigil-docs/`. |

---

## 6. Reproduce commands (authoritative)

```bash
# Checkout the reviewed commit (on branch feat/wasm-in-memory-api)
git -C sigil fetch origin feat/wasm-in-memory-api
git -C sigil worktree add /tmp/sigil-d0f04e2 d0f04e2
ln -sf /mnt/data/Workspace/Projects/vectomancy/vectomancy /tmp/vectomancy  # path-dep workaround for /tmp worktrees

# Lint & tests (as claimed in d0f04e2 commit message)
cargo fmt --check --manifest-path /tmp/sigil-d0f04e2/Cargo.toml
cargo clippy --manifest-path /tmp/sigil-d0f04e2/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path /tmp/sigil-d0f04e2/Cargo.toml --all-targets --features learned,c2pa -- -D warnings
cargo test --manifest-path /tmp/sigil-d0f04e2/Cargo.toml
cargo test --manifest-path /tmp/sigil-d0f04e2/Cargo.toml --features learned,c2pa  # ~44s, needs disk for linking

# WASM gate (lib only — bin intentionally fails on wasm32)
cargo check --manifest-path /tmp/sigil-d0f04e2/Cargo.toml --lib --target wasm32-unknown-unknown
cargo tree --manifest-path /tmp/sigil-d0f04e2/Cargo.toml --target wasm32-unknown-unknown | grep -E 'clap|glob|tracing-subscriber|rayon' | head -n 20
cargo tree --manifest-path /tmp/sigil-d0f04e2/Cargo.toml --target wasm32-unknown-unknown | wc -l  # 330

# This commit's isolated diff (not the whole branch vs main)
git -C sigil show d0f04e2 --stat
git -C sigil show d0f04e2 -- src/carrier.rs src/core.rs src/lib.rs src/embed.rs tests/c2pa_tests.rs

# Branch pollution check
git -C sigil log --oneline main..d0f04e2  # shows cf5f790... pollution
git -C sigil diff main..d0f04e2^ --stat   # 11 files / 384 insertions are CTX-0015, not CTX-0018
```

---

## 7. Conclusion

`d0f04e2` **passes** its stated verification (fmt/clippy/test/wasm `--lib`) and is **backwards compatible** and **behaviour-preserving** (pure delegation). Trait design is **minimal** and correctly prepares `sigil-core` extraction (DEC-0003 Phase 1). Dispatch is correct for DCT; DWT dispatch is correct for embed but `verify` ignores placement — flagged as P1. The only blocking issue is **branch pollution** (CTX-0015 protocol v2 commits underneath CTX-0018) which prevents clean cherry-pick to `main` and should be resolved by rebase or sequencing before merge. WASM leakage (`clap`/`glob`) remains — already tracked as CTX-0030, not introduced by this commit.

**Recommendation: Approve with comments** — merge after CTX-0015 sequencing / rebase and after CTX-0030 gating (or alongside it). No new correctness defects blocking the carrier abstraction itself.

*Reviewer: opencode — CTX-0028 — 2026-08-31*
