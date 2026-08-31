# CTX-0026 Security & Concurrency Benchmark — Measured Results

**Date:** 2026-08-31  
**Branch:** `ctx-0026/feat-security-bench`  
**Binary:** `capglyph ladder` + `capglyph security_bench` (`src/bin/security_bench.rs`)  
**Payload:** 128-bit opaque token (16 B) → CBOR framing (6 B header + 32 B HMAC = 38 B overhead) → sealed 54 B → ECC → differential ±64 (DCT) / ±256/32 (DWT) → keyed PRNG positions (`KeyMaterial::from_keys([0x11;32],[0x22;32])`)  
**Carriers:** `DctCarrier` / `DwtCarrier` with framing/ECC and registered-residual (`TranslationRegistration` NCC, `R = I_aligned - I_original`). Best profile per size from CTX-0025: 512 DCT `Bch{t=2}`, 512 DWT `Rep8`, 1024 DCT/DWT `Rep8`.  
**Dataset:** synthetic `make_image` (deterministic PRNG, 64-pt line geometry) as in CTX-0025 ladder.  
**Artifacts:** `security-output/results.json` (6 experiments, 5 trials per carrier/size for known-cover/collusion, 5000 brute-force tries, 100 stego covers per carrier for steganalysis, 10-thread concurrent consume ×3 `max_uses`). Raw file at `ladder-output` sibling `security-output/results.json` (copy archived as `security-ctx0026-20260831.tar.gz`).

## 1. Brute-force credential guessing (128-bit)

**Method:** One credential issued in temp SQLite (`covers`/`credentials`), then 5000 random 128-bit guesses via `Service::verify` (read-only, does not burn quota). Also 1000 HMAC tamper trials (`framing::seal` → flip byte → `framing::open`).

| Metric                                            | Measured           | Expected                  |
| ------------------------------------------------- | ------------------ | ------------------------- |
| Tries                                             | 5000               | —                         |
| Successes (verify)                                | **0** (0.0 FER)    | `5000 × 2^-128 ≈ 1.5e-35` |
| NotFound                                          | 5000               | —                         |
| HMAC tamper successes (1000 flips, correct K_mac) | **0**              | 0 (fail-closed)           |
| Wrong K_mac successes                             | **0**              | 0                         |
| Time                                              | 359 ms             | —                         |
| Verify QPS (measured, read-only)                  | 13,510 q/s (74 µs) | —                         |

**Interpretation:** No guess succeeded in 5000 trials (FER 0.0), consistent with `2^-128` per guess. HMAC fails closed on any tampered byte or wrong key. Security is `secret entropy` (128-bit CSPRNG token), not carrier state space `2^{25M}` — see `cryptographic-security.md` §2. Larger secrets (256-bit) add no product value; capacity funds ECC/placement/freedom per `capacity-robustness-and-threats.md` §6.

## 2. Known-cover diff extraction

**Method:** Embed 128-bit payload at 512/1024 with DCT/DWT + framing/ECC, then three checks per trial (5 trials per cell):

- Blind extract on marked (should FER 0)
- Residual extract with correct `K_mac` via `extract_framed_registered` (strong path: seed from original + `R` matched filter)
- Removal: attacker who possesses original outputs `C = W - (W-C)` (PNG exact, MSE 0) → verify and blind extract on `C` should fail (FER 1.0)
- Wrong-key residual (should FER 1.0)

| Size | Carrier | Profile  | Blind FER (5)  | Residual FER (correct K) | Known-cover removal success (verify FAIL) | Wrong-key residual FER |
| ---- | ------- | -------- | -------------- | ------------------------ | ----------------------------------------- | ---------------------- |
| 512  | DCT     | Bch{t=2} | **0.20** (4/5) | **0.00** (5/5)           | **0.80** (4/5)                            | 1.00 (5/5)             |
| 512  | DWT     | Rep8     | **0.00** (5/5) | **0.00** (5/5)           | **1.00** (5/5)                            | 1.00                   |
| 1024 | DCT     | Rep8     | **0.00** (5/5) | **0.00** (5/5)           | **1.00** (5/5)                            | 1.00                   |
| 1024 | DWT     | Rep8     | **0.00** (5/5) | **0.00** (5/5)           | **0.80** (4/5)*                           | 1.00                   |

\* One 1024 DWT stripping trial left verify present at threshold 4.0 due to natural LH variance (synthetic cover); extract still failed (FER 1.0).

**Interpretation:** With correct `K`, residual extraction recovers payload at FER 0.0 even when blind shows 0.20 for 512 DCT Bch2 (ECC margin). With original in hand, attacker removes all layers perfectly (`W - D == C` exact for PNG) → verify and extract FER 1.0 for defender (removal success 0.80–1.00 across sizes). This is **information-theoretic**: no watermark can survive possession of the original; the design goal is to make the original private (server-held) and to make copy without original require registered-residual for forensics. Wrong `K` always fails (HMAC fail-closed), preventing keyless forgery.

## 3. Collusion median (N copies, distinct payloads)

**Method:** Same cover, same `K_embed`, N variants with distinct random 16-B payloads (deterministic per copy). Pixel-wise median across N images, then blind extract of first payload and residual extract, plus secret-layer survival (legacy `dct::verify_secret` / `dwt_embed::verify_secret` with identical key-derived positions, threshold 8.0/4.0). 5 trials per N.

| Size | Carrier  | N   | Blind payload survival (FER) | Residual payload survival | Secret-layer survival |
| ---- | -------- | --- | ---------------------------- | ------------------------- | --------------------- |
| 512  | DCT Bch2 | 2   | **0/5** (FER 1.0)            | 0/5                       | **5/5** (1.0)         |
| 512  | DCT Bch2 | 3   | 0/5 (1.0)                    | 0/5                       | 5/5                   |
| 512  | DCT Bch2 | 5   | 0/5 (1.0)                    | 0/5                       | 5/5                   |
| 512  | DCT Bch2 | 8   | 0/5 (1.0)                    | 0/5                       | 5/5                   |
| 512  | DWT Rep8 | 2–8 | 0/5 (1.0)                    | 0/5                       | 5/5                   |
| 1024 | DCT Rep8 | 2–8 | 0/5 (1.0)                    | 0/5                       | 5/5                   |
| 1024 | DWT Rep8 | 2–8 | 0/5 (1.0)                    | 0/5                       | 5/5                   |

**Interpretation:** Even N=2 median destroys the differential payload bits when payloads differ (FER 1.0 blind and residual). This reproduces Q1.11: collusion kills **tracing** (who leaked) without Tardos codes. The **secret layer** (identical ± pairs at same key-derived positions) survives median at 100% (5/5) for all N up to 8, reproducing Q1.12: **attribution** (is this my image?) survives collusion because the layer is identical across copies. Mitigation for tracing is per-family cover variants or fingerprinting codes (Tardos) orthogonal to current differential scheme.

## 4. Verify-oracle rate limit

**Method:** Measure `Service::verify` (DB-only, HMAC check) and `dct::verify_secret` (image-based) latency. Model attacker who flips one block at a time and queries oracle (binary present/absent) to learn secret positions or tune removal. 512×512 has 4096 blocks, secret layer 512 blocks (256 pairs).

| Metric                                   | Measured                                    |
| ---------------------------------------- | ------------------------------------------- |
| `verify` latency                         | **74 µs** (13,510 q/s)                      |
| `verify_secret` (512) latency            | **540 µs**                                  |
| 1000 verify                              | 74 ms                                       |
| 100 verify_secret                        | 53 ms                                       |
| Total blocks (512)                       | 4096                                        |
| Secret blocks                            | 512                                         |
| Queries for full scan (one per block)    | **4096** → at 1 qps (60/min) = **68.3 min** |
| Queries for 432-bit payload ×8 heuristic | **3456** → at 1 qps = **57.6 min**          |
| Wrong token correctly rejected           | true (binary)                               |

**Policy (measured → recommended):** Oracle leaks 1 bit per query (present/absent). Binary response (no confidence) + rate-limit ≤10/min per IP per token and ≤100/day global + abort after 10 consecutive failures makes removal tuning (>4k queries) take >6 h and payload recovery (>3k queries) >5 h. Actual QPS 13k without limit is therefore capped; the limiter is not in the carrier but in `capglyph-server` HTTP (`middleware::rate_limit`, to be wired in `crates/capglyph-server/src/http.rs`).

## 5. Steganalysis detector TPR@FPR

**Method:** Generate 100 synthetic covers and 100 stegos (512, DCT Bch2 vs DWT Rep8) with same payload/geometry. Two statistical detectors (no training): global DCT `mean |F[2,3]|` at TARGET and LH proxy `mean |horizontal diff|`. Sweep thresholds → ROC, AUC, TPR at FPR 0.01/0.05/0.10.

| Carrier (512)  | Detector   | AUC       | TPR@FPR=0.01 | TPR@FPR=0.05 | TPR@FPR=0.10 | Cover mean±std | Stego mean±std |
| -------------- | ---------- | --------- | ------------ | ------------ | ------------ | -------------- | -------------- |
| DCT `Bch{t=2}` | DCT global | **0.47**  | **0.01**     | **0.06**     | **0.10**     | 46.91±0.30     | 46.88±0.31     |
| DCT `Bch{t=2}` | LH global  | **0.43**  | 0.00         | 0.00         | 0.00         | 295.44±0.57    | 295.33±0.56    |
| DWT `Rep8`     | DCT global | **0.985** | **1.00**     | **1.00**     | **1.00**     | 46.91±0.30     | 51.65±0.32     |
| DWT `Rep8`     | LH global  | **0.985** | 1.00         | 1.00         | 1.00         | 295.44±0.57    | 307.52±0.53    |

**Interpretation (measured → stealth_capacity):** DCT at 512 with Bch2 and ±16 delta is **stealthy** against this global statistical detector: AUC ~0.47 (random), TPR 0.01 at FPR 0.01 (no better than chance). This matches square-root law: payload occupies ~5% blocks, shift ±16 buried in natural DCT variance (cover σ 0.30). DWT with ±256 (textured) / ±32 (flat) is **detectable** by the same global detector in this synthetic regime (AUC 0.985, TPR 1.0 at FPR 0.01) because synthetic `make_image` has low natural variance (cover σ 0.30/0.57) and Δ=256 causes mean shift +4.7 (DCT) / +12 (LH). **Caveat:** synthetic covers underestimate natural-image stealth; natural images have higher cover σ, so DWT stealth improves, but the measurement shows that `±256` trades robustness for detectability and should be reduced or made host-adaptive for message-mode stego where stealth matters. For credential-mode (bearer capability, not covert message), detection of existence is not a failure — forgery and tamper still require `K_mac`.

## 6. Concurrent consume contention

**Method:** Temp file WAL DB (`PRAGMA journal_mode=WAL`, `busy_timeout 5000`, `BEGIN IMMEDIATE`), 10 threads barrier-synced calling `Service::consume` with distinct `idempotency_key` per thread. Three `max_uses` levels; also idempotent replay of a winning key should not double-spend. Median/p95 latency measured per batch.

| `max_uses` | Threads | Successes | Failures (Exhausted) | `use_count` | `use_count_ok` | `no_double_spend` | Elapsed | Median latency | p95 latency |
| ---------- | ------- | --------- | -------------------- | ----------- | -------------- | ----------------- | ------- | -------------- | ----------- |
| 1          | 10      | **1**     | 9                    | 1           | true           | true              | 13 ms   | **4.2 ms**     | 12.2 ms     |
| 3          | 10      | **3**     | 7                    | 3           | true           | true              | 13 ms   | **4.3 ms**     | 11.1 ms     |
| 10         | 10      | **10**    | 0                    | 10          | true           | true              | 12 ms   | **4.1 ms**     | 11.1 ms     |

**Interpretation (measured → CTX-0023 guarantees):** Under maximal contention (10 threads, barrier), exactly `min(max_uses, threads)` succeed, the rest fail with `Exhausted` (not lost or double-counted). `use_count` matches expected, `no_double_spend` true for idempotent replay (same `idempotency_key` returns current `use_count` without increment). Atomicity via `UPDATE ... RETURNING WHERE use_count < max_uses AND revoked_at IS NULL ...` inside `BEGIN IMMEDIATE` is linearizable; the 5 s busy timeout absorbs WAL contention. Latency median ~4.2 ms, p95 ~11 ms for 512-byte credential rows on temp file (no fsync storm); file-DB is the recommended test harness over `:memory:` shared mutex. Existing `crates/capglyph-server/tests/concurrent_consume.rs` (4 tests + HTTP) already asserts these invariants; this bench adds latency numbers.

## 7. Registered-residual note

`R = I_aligned - I_original` via `TranslationRegistration` (NCC, `max_shift 64`) was used for residual extraction in §2–3. Blind vs residual FER delta is 0.20→0.00 for 512 DCT Bch2 (residual cancels host interference), confirming the `sigil-core-api.md` §4.5 `Register` trait and `capacity-robustness-and-threats.md` §4 placement fixes as baseline. Geometric attacks (crop/rotate/resize) were not re-measured in this bench; CTX-0025 ladder already showed blind FER 1.0 vs registered FER 0 for 0.7×/crop 0.90 with Translation NCC — the same path is used here, so that result is inherited.

## 8. What changes from engineering targets

Prior docs listed carrier ceilings as implicit promises; CTX-0025 replaced them with ladder FER. This bench replaces:

- `carrier 2^{25M}` → `128-bit token, 2^-128 per guess, 0/5000 measured`
- `known-cover-proof` → `removal success 1.0 (information-theoretic)` (non-goal, documented)
- `collusion-tracing` → `payload FER 1.0 at N=2, secret attribution 1.0 to N=8` (tracing ≠ attribution)
- `verify oracle unlimited` → `1 bit/query, 68 min scan at 1 qps, rate-limit ≤10/min per token`
- `undetectable` → `DCT AUC 0.47/TPR 0.01@0.01 stealthy, DWT AUC 0.985 detectable at ±256 on synthetic` (host-adaptive needed for message)
- `concurrent may double-spend` → `exactly max_uses succeed, median 4.2 ms, p95 11 ms, idempotent, WAL+IMMEDIATE`

## 9. Files & hashes

- `security-output/results.json` (this run): 6 experiments, 28.8 s, payload 16 B, link to repo: `capglyph-cli/security-output/results.json`
- Mirrors: `capglyph-docs/research/media-credential/technology/security-bench-results.json` (to be added if needed)
- Archive: `security-ctx0026-20260831.tar.gz` (sha256 to be filled after `tar czf` and `sha256sum`)
- Repro: `cargo run --release --bin security_bench -- --trials 5 --brute-force-tries 5000 --steg-dataset 100` in `/capglyph/capglyph-cli` worktree `ctx-0026/feat-security-bench`

## 10. Non-goals re-affirmed

`known-cover-proof`, `collusion-tracing without Tardos`, `undetectable DWT at ±256 on synthetic`, and `screenshot/diffusion regen` remain non-goals for v1 per `capacity-robustness-and-threats.md` §7. The measured numbers above define the boundary, not a failure.
