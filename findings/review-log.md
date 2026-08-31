# Review Log

Continuous independent-review ledger for `sigil`. Every delivery task requires a different-agent `APPROVE` before merge; defects are recorded in CarryCtx progress/risk, appended here, and converted to follow-up `CTX-XXXX` tasks. See `.carryctx/rules/delivery.md` and `.carryctx/workflows/issue-to-merge.md` for lifecycle spec: GitHub Issue → CarryCtx task → branch/worktree `ctx-XXXX/<type>-<slug>` → commits → PR → independent review + CI → merge → docs sync → checkpoint → Issue close.

## How to use

1. Append one row per review (newest first). Keep `findings/YYYY-MM-DD-<slug>-review.md` for full detail when needed and link it.
2. Verdict: `APPROVE` / `REQUEST_CHANGES` / `COMMENT`. Include follow-up tasks for every blocking finding.
3. Mirror to `sigil-docs/findings/review-log.md` when `sigil-docs` was touched in the same PR.

| Date       | Task     | PR   | Reviewer (agent)             | Scope / Branch                                                                                                             | Verdict               | Findings / Detail                                                                                                                              | Follow-up CTX                                    | Docs sync                 |
| ---------- | -------- | ---- | ---------------------------- | -------------------------------------------------------------------------------------------------------------------------- | --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------ | ------------------------- |
| 2026-08-31 | CTX-0028 | —    | opencode (reviewer subagent) | `src/lib.rs`, `src/core.rs`, `src/carrier.rs`, `src/embed.rs` @ `d0f04e2` (CTX-0018)                                       | APPROVE with comments | See `findings/2026-08-31-carrier-review.md` §§2.1–2.9; DWT `verify` ignores placement, branch pollution (CTX-0015), `clap`/`glob` wasm leakage | CTX-0030, CTX-0031, CTX-0032, CTX-0033, CTX-0034 | n/a                       |
| 2026-08-31 | CTX-0027 | —    | opencode (reviewer subagent) | crate-split spike `findings/2026-08-31-crate-split-spike.md`                                                               | COMMENT               | Spike review: `findings/2026-08-31-crate-split-spike-review.md`; wasm ~330 nodes, `clap`/`glob` + `ravif` leakage, extraction path viable      | CTX-0030, CTX-0031                               | n/a                       |
| 2026-08-31 | CTX-0029 | #NNN | opencode                     | `.carryctx/rules/delivery.md`, `.carryctx/workflows/issue-to-merge.md`, `.carryctx/config.toml` (`ctx-XXXX/<type>-<slug>`) | PENDING               | Establish continuous review hygiene (this task)                                                                                                | —                                                | sigil-docs mirror pending |

<!-- Append new rows above the comment; keep header stable for `yamllint`/`markdownlint`. -->
