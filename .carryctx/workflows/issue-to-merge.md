# Issue-to-merge workflow

## Purpose

Use this workflow for repository changes after a GitHub Issue identifies the outcome. It defines durable CarryCtx mapping, isolated delivery, independent acceptance, and documentation synchronization. Lifecycle spec (normative delivery path) is: **GitHub Issue → CarryCtx task (CTX-XXXX) → team/dependencies/scopes → named session → isolated worktree/branch `ctx-XXXX/<type>-<slug>` → coherent commits → pull request → independent review (different agent, `APPROVE`) + CI → merge → docs sync verification → checkpoint → task completion → Issue closure**. Every delivery task requires independent review before merge. Branch naming and review hygiene are enforced via `.carryctx/rules/delivery.md` and this workflow.

## 1. Establish intent

1. Open or link a GitHub Issue with outcome, scope, constraints, security and performance impact, documentation impact, and acceptance evidence. Reference the CarryCtx task (`CTX-XXXX`) in the Issue body.
2. Confirm the work belongs to this repository. Split cross-repository work into linked Issues with explicit ordering.
3. Resolve or identify the accepted requirement, spec, ADR, or normative security control. Do not implement an undecided public boundary.

## 2. Encode execution in CarryCtx

1. Create or select the CarryCtx task linked to the Issue (`carryctx task create/show CTX-XXXX`).
2. Assign its team, required persona, owner, priority, and strong or informational dependencies (`carryctx task claim/start`).
3. Add exact file scopes and inspect conflicts. Serialize overlapping contracts.
4. Bind a named agent session and read team context before changes (`carryctx agent current --agent <name>`, `carryctx resume`).
5. Record progress, decisions, risks, blockers, handoffs, and checkpoints during the work (`carryctx progress`, `carryctx checkpoint create`). Defects discovered later are also recorded here.

## 3. Select workspace isolation

1. After the repository has a first commit, create a dedicated CarryCtx worktree and branch for parallel or substantial work: `carryctx worktree create CTX-XXXX --branch ctx-XXXX/<type>-<slug> --base main`. Branch naming is mandatory: `ctx-XXXX/<type>-<slug>` where `XXXX` is the zero-padded task number, `<type>` is `feat|fix|chore|docs|refactor|perf|test|build|ci`, and `<slug>` is kebab-case. Config `branch_template` is `ctx-{task_id}/{slug}` with `slug` already containing `<type>-<slug>`. Worktree path is `.worktrees/ctx-XXXX/`.
2. Keep one task outcome per branch and preserve unrelated changes. Verify with `git worktree list` and `carryctx doctor`.
3. In an unborn repository, branch/worktree/commit/PR stages are unavailable. The commander may authorize a shared checkout only when scopes are disjoint and local CI-equivalent checks are possible.
4. End the unborn exception after repository initialization.

## 4. Implement and verify

1. Adopt the assigned persona and applicable delivery, documentation, security, and performance rules (see `.carryctx/rules/`).
2. Implement only the accepted contract in scope. Add focused tests first where practical, then cover errors, bounds, denial, cleanup, and recovery.
3. Update affected canonical `sigil-docs` material for architecture, security, public behavior, configuration, compatibility, and developer workflows **in the same PR**. Documentation synchronization is part of Definition of Done — stale docs block `APPROVE`. Prefer linking to one authoritative definition instead of copying contracts into comments/tests.
4. Run focused checks and the repository integration gate. Minimum local gates before PR: `cargo fmt -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, plus `yamllint -d relaxed .` where YAML exists and `carryctx doctor`. Where WASM is touched, also `cargo check --target wasm32-unknown-unknown` and `cargo tree --target wasm32-unknown-unknown`. Record exact commands, environment, results, and residual gaps in CarryCtx progress.
5. Checkpoint a coherent milestone before handoff or task switching (`carryctx checkpoint create`).

## 5. Commit and open a pull request

1. Review the diff for scope, generated artifacts, secrets, accidental API expansion, and stale documentation. Ensure `findings/review-log.md` will be updated post-review if findings exist.
2. Create coherent commits linked to the Issue and CarryCtx task when commit authority exists. Commit messages reference `CTX-XXXX` and use `type(scope): subject` (e.g. `chore(review): establish continuous review hygiene (CTX-0029)`).
3. Open a pull request that states outcome, contracts, security/performance impact, docs synchronization, verification, dependencies, and merge order. PR description links the GitHub Issue (`Closes #NNN`) and CarryCtx task, and states the branch name (`ctx-XXXX/<type>-<slug>`). Include validation evidence and the exact docs revision.
4. Link affected cross-repository pull requests and the exact `sigil-docs` revision where applicable. CI must be green before review can `APPROVE`.

## 6. Independent review and CI

1. Move the task to review; the implementer does not self-accept. Reviewer must be a **different agent** than the implementer.
2. The reviewer reads the authoritative contracts, inspects the diff, reruns relevant checks (fmt/clippy/test/wasm), and records findings or explicit `APPROVE` / `REQUEST_CHANGES` in the PR and in CarryCtx progress/risk. Required geometry, signal, wasm, c2pa, performance, test, and docs owners review changes crossing their boundaries.
3. Defects, observations, and risks are recorded durably in CarryCtx progress/risk **and** appended to `findings/review-log.md` (and `sigil-docs/findings/review-log.md` when docs are affected). Each entry notes date, task, reviewer, scope, verdict, and disposition. Blocking findings are converted to follow-up `CTX-XXXX` tasks with team/priority/dependencies before merge; non-blocking observations may be tracked as `informational` follow-ups.
4. Resolve every blocking finding and CI failure before merge. Re-request review after fixes; the same independence rule applies to re-review.

## 7. Merge and close

1. Merge only after independent `APPROVE`, required CI passing, synchronized docs, and cross-repository ordering are satisfied. Preferred paths are `gh pr merge --squash` (delivery) or `gh pr merge --squash --auto` when policy allows; do not merge with unresolved `REQUEST_CHANGES`.
2. Verify the merged revision and any pinned documentation relationship (`git log --oneline`, `git show`, docs link).
3. Record final evidence and merged revision in a CarryCtx checkpoint; append the review outcome to `findings/review-log.md` if not already done.
4. Complete the CarryCtx task (`carryctx task complete CTX-XXXX`) and close the GitHub Issue (`gh issue close`). Preserve follow-up work as linked `CTX-XXXX` tasks. End the session checkpoint per `require_before_session_end`.
