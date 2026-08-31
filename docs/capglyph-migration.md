# CapGlyph Filesystem Isolation (CTX-0037)

**Date:** 2026-08-31
**Task:** CTX-0037
**Epic:** CTX-0035 (DEC-0004)
**Branch:** `ctx-0037/chore-filesystem-isolation`
**Base:** `main` (7136617)

Isolated filesystem migration of `vectomancy/sigil*` monorepo layout to
`/mnt/data/Workspace/Projects/capglyph/` standalone org layout.

## Source → Target Mapping

| Source (vectomancy) | Target (capglyph isolated) | Method | GitHub Remote | Notes |
|---|---|---|---|---|
| `vectomancy/sigil` | `/mnt/data/Workspace/Projects/capglyph/capglyph-cli` | `gh repo clone CapGlyph/capglyph-cli` (fresh clone from transferred repo) | `origin=https://github.com/CapGlyph/capglyph-cli.git` `upstream=https://github.com/CapGlyph/capglyph-cli.git` (legacy location now redirects) | Legacy Sigil repo already transferred → CapGlyph/capglyph-cli (verified `gh api` returns `full_name=CapGlyph/capglyph-cli`). HEAD `7136617` identical. History preserved. |
| `vectomancy/sigil-docs` | `/mnt/data/Workspace/Projects/capglyph/capglyph-docs` | `gh repo clone CapGlyph/capglyph-docs` | `origin=https://github.com/CapGlyph/capglyph-docs.git` `upstream=https://github.com/CapGlyph/capglyph-docs.git` | Private repo, HEAD `40d755b` identical to source. |
| `vectomancy/sigil/crates/sigil-core` | `/mnt/data/Workspace/Projects/capglyph/capglyph-core` | `gh repo clone CapGlyph/capglyph-core` (empty placeholder) | `origin=https://github.com/CapGlyph/capglyph-core.git` `upstream=https://github.com/CapGlyph/capglyph-cli.git` | Empty repo (CTX-0040 will extract `crates/sigil-core` as standalone canonical Rust Core with WASM/FFI). |
| `vectomancy/sigil-paper` | `/mnt/data/Workspace/Projects/capglyph/capglyph-paper` | `cp -a vectomancy/sigil-paper → capglyph/capglyph-paper` | `origin=https://github.com/CapGlyph/capglyph-paper.git` (created 2026-08-31, private) | Copied `.git` + `carryctx/state.sqlite` preserving `SIGILP` tasks. Original has no remote; new remote created via `gh repo create CapGlyph/capglyph-paper --private`. Push pending. |
| — | `/mnt/data/Workspace/Projects/capglyph/capglyph-spec` | `gh repo clone CapGlyph/capglyph-spec` (empty) | `origin=https://github.com/CapGlyph/capglyph-spec.git` | Created CTX-0036, reserved for CTX-0041. |
| — | `/mnt/data/Workspace/Projects/capglyph/capglyph-test-vectors` | `gh repo clone CapGlyph/capglyph-test-vectors` (empty) | `origin=https://github.com/CapGlyph/capglyph-test-vectors.git` | Created CTX-0036, reserved for CTX-0041. |

**Verified 2026-08-31:** `ls /mnt/data/Workspace/Projects/capglyph/` shows 6 entries (cli, core, docs, paper, spec, test-vectors).

## Remotes (verified `git -C <new> remote -v`)

```text
capglyph-cli:
origin	https://github.com/CapGlyph/capglyph-cli.git (fetch/push)
upstream	https://github.com/CapGlyph/capglyph-cli.git (fetch/push)

capglyph-docs:
origin	https://github.com/CapGlyph/capglyph-docs.git
upstream	https://github.com/CapGlyph/capglyph-docs.git

capglyph-core:
origin	https://github.com/CapGlyph/capglyph-core.git
upstream	https://github.com/CapGlyph/capglyph-cli.git

capglyph-paper:
origin	https://github.com/CapGlyph/capglyph-paper.git

capglyph-spec / capglyph-test-vectors:
origin	https://github.com/CapGlyph/capglyph-*.git
```

All `origin` use HTTPS per project convention (previously SSH for fresh clones, updated via `git remote set-url`).

Upstream preserves `vectomancy` history (legacy Sigil repos, now CapGlyph). Legacy locations now redirect to CapGlyph (transfer verified via `gh api repos/CapGlyph/capglyph-cli → full_name CapGlyph/capglyph-cli`).

## CarryCtx Verification (`carryctx --project <new> doctor`)

```text
capglyph-cli:    ✓ Git repository, ✓ Database at .git/carryctx/state.sqlite, ✓ Schema up to date, ✓ No orphaned tasks, ✓ No pending worktree cleanups — Everything looks good! (hooks not installed → Fix: carryctx hooks install)

capglyph-docs:   ✓ same — Everything looks good!

capglyph-paper:  ✓ Git repository, ✓ CarryCtx hooks installed, ✓ Database, ✓ Schema, ✓ 1 active session — Everything looks good! (state.sqlite copied via cp -a, project id 01M0AT4Q...)

capglyph-core:   ✓ Git repository (empty), ✓ Database, ✓ Schema — Everything looks good!
```

- `vectomancy/sigil` project `01M0094ARQJD79NQ9M0P1YSWPZ` (CTX prefix) retains full task DB (42 tasks, CTX-0037 in_progress, worktree `ctx-0037/chore-filesystem-isolation` at `.worktrees/ctx-0037`).
- `capglyph-cli` clone shares config `01M0094...` but has fresh empty state (no tasks) — intentional isolation; future work in capglyph layout will init own tasks. Alternative is to copy `state.sqlite` — deferred, as current worktree tracking remains in vectomancy/sigil.
- `vectomancy/sigil-docs` (`01M0HC...`, SIGILD) and `sigil-paper` (`01M0AT...`, SIGILP) similarly preserved.
- Worktree isolation respects monorepo vs isolated monorepo: `vectomancy/.git` and `capglyph/*/ .git` are distinct; `carryctx --project` must be explicit for each.

## Filesystem Layout (after)

```text
/mnt/data/Workspace/Projects/vectomancy/          # legacy monorepo container (not a git repo)
├── sigil/                 → origin CapGlyph/capglyph-cli (now redirect), 7136617
├── sigil-docs/            → origin CapGlyph/capglyph-docs (redirect), 40d755b
├── sigil-paper/           → no remote, cefd342
├── sigil-website/
├── vectomancy/            (core engine)
└── topoglyph/

/mnt/data/Workspace/Projects/capglyph/             # isolated org container (each subdir is independent git repo)
├── capglyph-cli/          → CapGlyph/capglyph-cli (7136617, tracks sigil)
├── capglyph-docs/         → CapGlyph/capglyph-docs (40d755b, private)
├── capglyph-core/         → CapGlyph/capglyph-core (empty, awaiting CTX-0040)
├── capglyph-paper/        → CapGlyph/capglyph-paper (private, cp -a from sigil-paper)
├── capglyph-spec/         → CapGlyph/capglyph-spec (empty, awaiting CTX-0041)
└── capglyph-test-vectors/ → CapGlyph/capglyph-test-vectors (empty)
/mnt/data/Workspace/Projects/vectomancy/sigil/.worktrees/ctx-0037 → worktree for this task
```

## Commands Executed (idempotent)

```bash
# 1. Worktree (tracking)
export CARRYCTX_AGENT=hermes
carryctx --project /mnt/data/Workspace/Projects/vectomancy/sigil session start --task CTX-0037
carryctx --project /mnt/data/Workspace/Projects/vectomancy/sigil task start CTX-0037
mkdir -p /mnt/data/Workspace/Projects/vectomancy/sigil/.worktrees
carryctx --project /mnt/data/Workspace/Projects/vectomancy/sigil worktree create CTX-0037 --path /mnt/data/Workspace/Projects/vectomancy/sigil/.worktrees/ctx-0037 --branch ctx-0037/chore-filesystem-isolation --base main

# 2. Isolated clones (HTTPS_PROXY required for gh)
HTTPS_PROXY=$NETWORK_PROXY gh repo clone CapGlyph/capglyph-cli /mnt/data/Workspace/Projects/capglyph/capglyph-cli
HTTPS_PROXY=$NETWORK_PROXY gh repo clone CapGlyph/capglyph-docs /mnt/data/Workspace/Projects/capglyph/capglyph-docs
HTTPS_PROXY=$NETWORK_PROXY gh repo clone CapGlyph/capglyph-core /mnt/data/Workspace/Projects/capglyph/capglyph-core
HTTPS_PROXY=$NETWORK_PROXY gh repo clone CapGlyph/capglyph-spec /mnt/data/Workspace/Projects/capglyph/capglyph-spec
HTTPS_PROXY=$NETWORK_PROXY gh repo clone CapGlyph/capglyph-test-vectors /mnt/data/Workspace/Projects/capglyph/capglyph-test-vectors
cp -a /mnt/data/Workspace/Projects/vectomancy/sigil-paper /mnt/data/Workspace/Projects/capglyph/capglyph-paper

# 3. Remotes to HTTPS + upstream
git -C /mnt/data/Workspace/Projects/capglyph/capglyph-cli remote set-url origin https://github.com/CapGlyph/capglyph-cli.git
git -C /mnt/data/Workspace/Projects/capglyph/capglyph-cli remote add upstream https://github.com/CapGlyph/capglyph-cli.git
git -C /mnt/data/Workspace/Projects/capglyph/capglyph-docs remote set-url origin https://github.com/CapGlyph/capglyph-docs.git
git -C /mnt/data/Workspace/Projects/capglyph/capglyph-docs remote add upstream https://github.com/CapGlyph/capglyph-docs.git
git -C /mnt/data/Workspace/Projects/capglyph/capglyph-core remote set-url origin https://github.com/CapGlyph/capglyph-core.git
git -C /mnt/data/Workspace/Projects/capglyph/capglyph-core remote add upstream https://github.com/CapGlyph/capglyph-cli.git

HTTPS_PROXY=$NETWORK_PROXY gh repo create CapGlyph/capglyph-paper --private --description "CapGlyph paper — academic paper repository for CapGlyph (migrated from sigil-paper)"
git -C /mnt/data/Workspace/Projects/capglyph/capglyph-paper remote add origin https://github.com/CapGlyph/capglyph-paper.git

# 4. Verify
ls -la /mnt/data/Workspace/Projects/capglyph/
git -C /mnt/data/Workspace/Projects/capglyph/capglyph-cli remote -v
git -C /mnt/data/Workspace/Projects/capglyph/capglyph-docs remote -v
git -C /mnt/data/Workspace/Projects/capglyph/capglyph-core remote -v
git -C /mnt/data/Workspace/Projects/capglyph/capglyph-paper remote -v
carryctx --project /mnt/data/Workspace/Projects/capglyph/capglyph-cli doctor
carryctx --project /mnt/data/Workspace/Projects/capglyph/capglyph-docs doctor
carryctx --project /mnt/data/Workspace/Projects/capglyph/capglyph-paper doctor
carryctx --project /mnt/data/Workspace/Projects/capglyph/capglyph-core doctor
```

## Next Steps (out of scope for CTX-0037)

- CTX-0038: code rename `sigil` → `capglyph` (crate, binary, env) — do not touch in this PR.
- CTX-0039: docs rename `Sigil` → `CapGlyph` (README, URLs).
- CTX-0040: extract `crates/sigil-core` into `capglyph-core` standalone repo.
- CTX-0041: populate `capglyph-spec` + `capglyph-test-vectors`.

## Worktree Isolation Note

`vectomancy` monorepo and `capglyph` isolated monorepo are distinct filesystem roots.
Each `capglyph-*` is an independent git repo with its own `.git/carryctx/state.sqlite`.
Do not `cp -a` the `.git` of one into another; use `gh repo clone` for CapGlyph remotes
and preserve history via `upstream` remote.

Task: CTX-0037
Closes CTX-0037
