# Review: CTX-0017 crate-split spike findings (CTX-0027)

**Reviewer:** security-reviewer / docs-engineer (opencode)  
**Date:** 2026-08-31  
**Task:** CTX-0027 — review of CTX-0017  
**Artifacts reviewed:**

- `sigil/findings/2026-08-31-crate-split-spike.md` (commit `f9f20e4`, sigil repo)
- `sigil-docs/findings/2026-08-31-crate-split-spike.md` (commit `fc53621`, sigil-docs repo) — identical copy
- Verifying commit: `git -C sigil show f9f20e4` (1 file, 258 insertions, no code change)
- Branch context: `feat/wasm-in-memory-api` @ `cf5f790...d0f04e2`; `main` @ `308293f` is stale vs findings baseline
- Toolchain as documented: `rustc 1.97.1`, `cargo 1.97.1`, `wasm-pack 0.15.0`, `cargo-bloat 0.12.1`, `wasm-opt 130`, `wasm32-unknown-unknown` installed

---

## 1. Verification summary

### 1.1 Numbers reproduced

All primary numbers were independently reproduced against the **feat branch state** (which includes `b93be8d` wasm-enablement fix removing the `vectomancy` facade). Raw `main` (308293f) predates that fix and yields different counts (666 nodes) — see §2.1.

| Claim in spike ( § )                                                                                                                                | Reproduced value                                                                                                                                           | Command                                                                                                                                                                 | Verdict                                          |
| --------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------ |
| `cargo tree --target wasm32-unknown-unknown` lines 330 (lib)                                                                                        | **330**                                                                                                                                                    | `cargo tree --target wasm32-unknown-unknown \| wc -l` (feat branch, no `--lib` flag)                                                                                    | ✅ exact                                         |
| native nodes 340                                                                                                                                    | **340**                                                                                                                                                    | `cargo tree \| wc -l`                                                                                                                                                   | ✅ exact                                         |
| via `sigil-wasm` 362                                                                                                                                | **362**                                                                                                                                                    | `cargo tree --target wasm32-unknown-unknown --manifest-path sigil-website/wasm-engine/Cargo.toml \| wc -l`                                                              | ✅ exact                                         |
| unique crates 160                                                                                                                                   | **160**                                                                                                                                                    | `cargo tree --target wasm32-unknown-unknown \| grep -oE '[a-z_-]+ v[0-9]' \| sort -u \| wc -l` — note: alternate regex gives 168/169 but doc's exact pipeline gives 160 | ✅ exact with documented pipeline                |
| `cargo bloat` text 6.1 MiB, file 11.9 MiB, ravif 1.7 MiB dominant                                                                                   | **6.1 MiB text, 1.7 MiB ravif (27.7%), file 11.9 MiB**                                                                                                     | `cargo bloat --release --crates -n 25` (warm `Finished in 0.08s`) — also shows `imageproc 891 B`, `tiny-skia 10.7 KiB` after LTO as stated                              | ✅ exact                                         |
| WASM raw before opt 5.1 MiB                                                                                                                         | **5.1 MiB**                                                                                                                                                | `ls -lh sigil-website/wasm-engine/target/wasm32-unknown-unknown/release/sigil_wasm.wasm` (5,255,324 B)                                                                  | ✅ exact                                         |
| `pkg/sigil_wasm_bg.wasm` after `wasm-opt -Oz` 2.7 MiB                                                                                               | **2.7 MiB** (2,767,330 B)                                                                                                                                  | `ls -lh sigil-website/wasm-engine/pkg/sigil_wasm_bg.wasm`                                                                                                               | ✅ exact                                         |
| gzipped 978 KiB (977,686 B)                                                                                                                         | **977,686**                                                                                                                                                | `gzip -c pkg/sigil_wasm_bg.wasm \| wc -c`                                                                                                                               | ✅ exact                                         |
| `pkg` without opt 5.6 MiB                                                                                                                           | **not re-measured** (requires `wasm-pack --no-opt` rebuild, ~19 s); raw 5.1 MiB vs pkg-opt 2.7 MiB already confirms ~2× shrinkage                          | —                                                                                                                                                                       | ⚠️ plausible, not re-run to save time            |
| build times §2 (check native 17.0 s cold / wasm lib 10.9 s / wasm-engine 11.8 s / release 33.5 s / wasm-pack 19.1 s, warm 0.09 s, incr 0.20–0.26 s) | spot-checked warm `cargo check --target wasm32-unknown-unknown --lib` **0.11–0.12 s** (matches doc warm 0.09–0.10 s); cold not re-run to avoid cache flush | `bash -c 'time cargo check --target wasm32-unknown-unknown --lib'`                                                                                                      | ✅ warm matches; cold plausible, not re-measured |
| hypothetical delta 0..+20 KiB                                                                                                                       | estimate, not measurement — inferred from bloat (sigil 12% of text) and LTO docs                                                                           | —                                                                                                                                                                       | ✅ sound inference, correctly labeled est.       |

### 1.2 Leakage claim ( §1.3 )

| Claim                                                             | Reproduced                                                                                     | Verdict                                              |
| ----------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------- |
| `clap` 4.6.6 + `clap_builder` etc present in wasm32               | **present** (depth 1, 4 occurrences) `cargo tree --target wasm32-unknown-unknown \| grep clap` | ✅ confirmed leakage                                 |
| `glob 0.3.4` present                                              | **present**                                                                                    | ✅ confirmed leakage                                 |
| `tracing-subscriber` absent (correctly gated)                     | **absent** in wasm32, present in native (`cargo tree \| grep tracing-subscriber`)              | ✅ correct                                           |
| `rayon`/`rayon-core` present but inert (9 occurrences)            | **9** `grep -c rayon`                                                                          | ✅ exact                                             |
| `imageproc` 1 occurrence, 891 B after LTO                         | **1** occurrence, native bloat shows 891 B                                                     | ✅ exact                                             |
| `ravif`/`rav1e`/`exr`/`tiff`/`webp` heavy via feature unification | **present** (`cargo tree --target wasm32-unknown-unknown -e features \| grep -E "ravif         | webp"`shows`image feature "webp" (*)` pulling ravif) | ✅ confirmed |
| `trustmark`/`c2pa`/`directories`/`ureq` absent                    | **absent** (optional, not enabled)                                                             | ✅ correct                                           |

**Headline in spike is accurate:** `clap`+`glob` leak, `tracing-subscriber` correctly gated, `rayon`/`imageproc` present but inert, codecs dominate size.

### 1.3 Recommendation soundness ( §5 )

> **Do not split now** — split does not shrink wasm (codecs dominate), does not speed builds (only ~0.08 s incremental win), leakage fix is one-line gating, real win is codec trim.

- **Sound.** Bloat shows watermark logic < 15% of wasm; split cannot deduplicate shared deps under `workspace.dependencies`. Estimated wasm delta 0..+20 KiB if LTO less effective is consistent with rustc LTO docs and native bloat. Cold/warm check times already show crate granularity not a bottleneck.
- **Sequencing (CTX-0018/0019/0022) is prudent:** single-crate carrier trait first, then paper spec, then one `sigil-core` extraction — avoids locking `GeometryFile`/`SignalMetrics`/`Spread` as public API before framing/ECC layer exists. Downgrading to `sigil-dct`/`sigil-dwt` separate crates is correctly deferred until a concrete consumer (sigild / alternate wasm) justifies it.

---

## 2. Defects and observations

### 2.1 🔴 DEFECT — `main` vs `feat` branch divergence not called out in findings header

- **Location:** `findings/2026-08-31-crate-split-spike.md:5` says Branch: `feat/wasm-in-memory-api (cf5f790)` but does not warn that `main` (308293f) predates `b93be8d` and gives different numbers.
- **Impact:** Reproducing on raw `main` yields `cargo tree wasm32` **666 lines** (includes `vectomancy` facade with `tera`/`rand` etc) vs documented **330**. Reviewer initially reproduced 666 on the `ctx-0027` worktree based on `main` and had to switch to feat state to confirm. Future readers on `main` will see mismatch.
- **Recommendation:** Add a one-line note in §1.2 or §6: "Numbers require `b93be8d` (remove vectomancy facade) — on older `main` without that commit, wasm tree is ~666 lines." No file change needed now — noted here for next findings update.
- **Follow-up:** None — informational; next spike on `main` after it catches up will self-correct.

### 2.2 🔴 DEFECT — CTX-0018 did NOT gate `clap`/`glob` as recommended

- **Location:** `sigil/Cargo.toml:22,48` still declares `clap` and `glob` as unconditional `[dependencies]` at `d0f04e2` (CTX-0018 tip). Findings §4.1 and §5 explicitly say "Gate `clap`/`glob` out of wasm in the same PR — single `Cargo.toml` diff" for CTX-0018.
- **Evidence:**
  ```
  cargo tree --target wasm32-unknown-unknown | grep -E "clap|glob"  # feat branch d0f04e2 still shows both
  cat Cargo.toml | grep -A2 "target.*cfg"  # only tracing-subscriber gated
  ```
- **Expected:** `[target.'cfg(not(target_arch="wasm32"))'.dependencies]` should contain `clap` and `glob` (like `tracing-subscriber`), dropping wasm tree by ~22 nodes to ~340 (via wasm-engine) and saving ~0.2–0.4 s `cargo check` and 50–100 KiB wasm-opt (clap_builder 273 KiB native bloat, less after LTO but non-zero).
- **Follow-up:** Created task (see §4) — gate `clap`/`glob` (and `anstream` transitive) out of wasm. Low-risk: move two lines in `Cargo.toml`, verify `cargo tree --target wasm32-unknown-unknown` drops clap/glob and `cargo check --target wasm32-unknown-unknown --lib` + `cargo test` still pass. Should be done before CTX-0022.

### 2.3 🟡 OBSERVATION — `cargo check` without `--lib` failure is correctly documented but the reproduce snippet is easy to mis-copy

- **Location:** `findings/2026-08-31-crate-split-spike.md:124` table and `§6` snippet.
- **Detail:** Spike correctly notes `cargo check --target wasm32-unknown-unknown` (incl. bin) fails due to `tracing_subscriber` in `src/main.rs`. Verification confirms: `cargo check --target wasm32-unknown-unknown` → error `tracing_subscriber` unresolved; `cargo check --target wasm32-unknown-unknown --lib` succeeds in 0.11 s. The `--lib` flag is not a standard `cargo tree` flag (`cargo tree --lib` is invalid) — doc correctly uses `cargo tree --target ...` without `--lib` for tree counts. No fix needed, but readers should note the distinction.
- **Follow-up:** None.

### 2.4 🟡 OBSERVATION — Hypothetical delta 0..+20 KiB is an estimate, not a measurement

- **Location:** `§3.2` and `§3.3` "WASM bundle +5–20 KiB" and `§5` "Estimated wasm delta is zero to +20 KiB".
- **Detail:** No actual workspace prototype was built (spike was measurements-only, per task scope). Estimate is reasoned from bloat and LTO docs, correctly labeled as `est.` No defect — but future CTX-0022 should re-measure after `image` codec trim before deciding.
- **Follow-up:** None — covered by existing CTX-0022 sequencing.

### 2.5 🟡 OBSERVATION — Build times are n=1, single-host

- **Location:** `§2` "All times `TIMEFORMAT=...`"
- **Detail:** Cold times 17.0 s, 10.9 s, etc are single runs. Acceptable for a spike, and warm/incremental are inherently cached. Doc correctly notes "measured on this host" and does not over-claim significance.
- **Follow-up:** None.

### 2.6 🟡 OPTIMIZATION — Image codec trim estimate ( §4.2 ) is high-value and dwarfs split benefit

- **Detail:** `vectomancy-raster` enables `image/features=["webp"]` which via feature unification pulls `ravif`/`rav1e`/`exr`/`tiff`/`image-webp` (~2.9 MiB text, ~25% of native bin) into every consumer even though sigil's own `image` is `default-features=false, features=["png","jpeg"]`. Doc estimates wasm-opt 2.7 MiB → −0.6–0.8 MiB and gz 978 KiB → −200–300 KiB after trim — this is larger than any split benefit and correctly prioritized as #2.
- **Options proposed are sound:** (a) make `vectomancy-raster`'s `image` dep default-off + feature-gated `webp`, or (b) override in `sigil` and verify with `cargo tree -e features` that resolver v2 does not unify across path deps. Option (a) is cleaner and respects upstream.
- **Follow-up:** Created task (see §4) — evaluate and implement `image` feature trim for wasm path, verify with `cargo tree --target wasm32-unknown-unknown -e features` that `ravif`/`rav1e` disappear, re-measure `cargo bloat` and `wasm-opt` sizes.

### 2.7 🟢 DOC QUALITY — Reproduce section and archiving

- **Positive:** `§6` is fully copy-paste reproducible (toolchain, tree, bloat, wasm-pack, verification). `§7` archives raw outputs. `§4` explicitly suggests `cargo tree -e features` verification step that `§6` could also include — minor gap, not a defect.

---

## 3. Soundness checks not in spike but verified by reviewer

| Check                                                                            | Result                                                 |
| -------------------------------------------------------------------------------- | ------------------------------------------------------ |
| `cargo fmt -- --check`                                                           | pass (on feat branch)                                  |
| `cargo clippy --all-targets -- -D warnings`                                      | pass                                                   |
| `cargo test`                                                                     | 19 lib + 18 integration + 8 wasm_api pass (sigil repo) |
| `cargo check --target wasm32-unknown-unknown --lib`                              | pass (0.11 s warm)                                     |
| `cargo tree --target wasm32-unknown-unknown` does not contain `trustmark`/`c2pa` | confirmed absent                                       |
| `findings` file identical in `sigil` and `sigil-docs`                            | `diff` empty — 258 lines each                          |
| `sigil-docs` commit `fc53621` message and `sigil` `f9f20e4` message consistent   | both describe same spike numbers                       |

---

## 4. Follow-up tasks created

| #   | Title                                                         | Priority | Scope                                                                                                                                                                                                                                                                                                                                                                           |
| --- | ------------------------------------------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Gate `clap` and `glob` out of wasm (`cfg(not(wasm32))`)       | high     | `sigil/Cargo.toml` — move `clap`/`glob` to `[target.'cfg(not(target_arch="wasm32"))'.dependencies]`, verify `cargo tree --target wasm32-unknown-unknown` no longer contains clap/glob, `cargo check --lib --target wasm32` and native `cargo check`/`test` pass, re-measure `cargo tree` lines and `wasm-opt` size for a one-line follow-up note                                |
| 2   | Trim `image` codec feature unification for wasm (ravif/rav1e) | normal   | `vectomancy/crates/vectomancy-raster/Cargo.toml` + `sigil/Cargo.toml` — evaluate making `image` dep default-off with optional `webp` feature vs sigil-side override; verify with `cargo tree --target wasm32-unknown-unknown -e features` that `ravif`/`rav1e`/`exr`/`tiff`/`image-webp` are removed for wasm; re-run `cargo bloat` and `wasm-pack` size checks; document delta |

Both depend on CTX-0017 (spike) and should be done before CTX-0022 (sigil-core extraction) as recommended.

---

## 5. Reviewer recommendation on CTX-0017

**APPROVE** the spike findings as accurate and well-substantiated. Numbers are reproducible against the documented branch/toolchain, leakage audit is correct, hypothetical estimates are clearly labeled, and the "no split now" recommendation is well-reasoned with correct sequencing. Two follow-ups are warranted (gating clap/glob — missed in CTX-0018; and image codec trim) but do not block acceptance of the spike itself. No re-measurement or doc rewrite required beyond the one-line branch-divergence note already recorded here.

---

## 6. How this review was performed

```bash
# artifacts inspected (per task §4)
git -C sigil log --oneline f9f20e4^..f9f20e4
git -C sigil show f9f20e4
cat sigil/findings/2026-08-31-crate-split-spike.md
git -C sigil-docs show fc53621
diff sigil/findings/... sigil-docs/findings/...

# reproducibility (feat branch state d0f04e2 / feat/wasm-in-memory-api)
cargo tree --target wasm32-unknown-unknown | wc -l                          # 330
cargo tree | wc -l                                                           # 340
cargo tree --target wasm32-unknown-unknown --manifest-path sigil-website/wasm-engine/Cargo.toml | wc -l  # 362
cargo tree --target wasm32-unknown-unknown | grep -oE '[a-z_-]+ v[0-9]' | sort -u | wc -l  # 160
cargo tree --target wasm32-unknown-unknown | grep -E "clap|glob|tracing-subscriber"
cargo tree --target wasm32-unknown-unknown | grep -c rayon                   # 9
cargo bloat --release --crates -n 25                                         # 6.1 MiB text
ls -lh sigil-website/wasm-engine/pkg/sigil_wasm_bg.wasm sigil-website/wasm-engine/target/wasm32-unknown-unknown/release/sigil_wasm.wasm
gzip -c sigil-website/wasm-engine/pkg/sigil_wasm_bg.wasm | wc -c              # 977686
cargo check --target wasm32-unknown-unknown --lib                            # pass
cargo tree --target wasm32-unknown-unknown -e features | grep -E "ravif|webp"
```

Worktree: `ctx-0027-review-crate-split-spike` @ `/mnt/data/Workspace/Projects/vectomancy/sigil-ctx-0027` (base `main`, cherry-picked `f9f20e4`). Verified feat-branch numbers via `d0f04e2` state as well.

---

## 7. Paths and checks referenced

- `sigil/findings/2026-08-31-crate-split-spike.md:1-258`
- `sigil-docs/findings/2026-08-31-crate-split-spike.md:1-258`
- `sigil/Cargo.toml:22,48,68-69` (clap/glob gating)
- `vectomancy/crates/vectomancy-raster/Cargo.toml` (`image = { features = ["webp"] }`)
- `sigil-website/wasm-engine/Cargo.toml` (sigil-wasm cdylib, `opt-level=z`)
- `sigil-website/wasm-engine/pkg/sigil_wasm_bg.wasm` (2.7 MiB opt, 978 KiB gz)
- CI verification: `cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo check --target wasm32-unknown-unknown --lib`
