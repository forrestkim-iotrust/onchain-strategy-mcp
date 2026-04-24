---
phase: 01-mcp-runtime-surface
plan: 01
subsystem: infra
tags: [rust, cargo-workspace, rmcp, schemars, tokio, clippy, edition-2024]

requires: []
provides:
  - 4-crate Cargo workspace (executor-mcp, executor-core, executor-state, executor-signer) on 2024 edition
  - rust-toolchain.toml pinned to stable with rustfmt + clippy
  - Workspace-level + crate-level deny lints for print_stdout / print_stderr / dbg_macro (D-05)
  - executor-core schema module tree with 7 JsonSchema-derived tool/prompt input structs and SignedTransaction placeholder
  - executor-core CoreError enum with an InvalidInput variant
  - executor-signer Signer trait boundary (empty, Phase 6 implements)
  - Wave 0 integration harness (tests/common/mod.rs with spawn_server/send/recv/initialize + stdio_handshake.rs entry file)
  - executor-core tests/schema_snapshots.rs golden snapshot scaffold + tests/schemas/ directory
affects:
  - 01-02-PLAN.md (tool/prompt router wiring will import executor_core::schema::* and add #[tokio::test] fns to stdio_handshake.rs)
  - 01-03-PLAN.md (resources/prompts + stdout discipline tests reuse the same harness)
  - Phase 2 (executor-state persistence will reuse CoreError + schema types)
  - Phase 5 (policy module replaces PolicyUpdateInput placeholder)
  - Phase 6 (executor-signer::Signer gains real methods; SignedTransaction gets rlp fields)

tech-stack:
  added:
    - rmcp 1.5 (workspace-level dep, bound in Plan 02)
    - schemars 1.2 (JsonSchema derive on all input structs)
    - serde 1 + serde_json 1
    - tokio 1 (process/io-util/time features in executor-mcp dev-deps)
    - tracing 0.1 + tracing-subscriber 0.3 (stderr logger wired in Plan 02)
    - thiserror 2 (CoreError)
    - anyhow 1 (integration-test helpers)
    - toml 0.8 (config loader wired in Plan 02)
  patterns:
    - Workspace.lints.clippy for stdout discipline, plus crate-level `#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]` in every crate's entrypoint
    - executor-core stays pure-domain (no rmcp dep) so persistence / signer / EVM crates can reuse it
    - Golden-file schema snapshot pattern (tests/schemas/<Name>.json with UPDATE_SCHEMAS env toggle)
    - Line-delimited JSON-RPC harness: spawn_server draining stderr on a background task, recv asserting every stdout line parses and carries `jsonrpc: "2.0"`

key-files:
  created:
    - Cargo.toml (workspace root, members + shared deps + workspace lints)
    - rust-toolchain.toml (stable + rustfmt + clippy)
    - .gitignore (/target)
    - crates/executor-core/Cargo.toml
    - crates/executor-core/src/lib.rs
    - crates/executor-core/src/error.rs
    - crates/executor-core/src/schema/mod.rs
    - crates/executor-core/src/schema/strategy.rs
    - crates/executor-core/src/schema/action.rs
    - crates/executor-core/src/schema/execution.rs
    - crates/executor-core/src/schema/policy.rs
    - crates/executor-core/src/schema/prompt_args.rs
    - crates/executor-core/tests/schema_snapshots.rs
    - crates/executor-core/tests/schemas/.gitkeep
    - crates/executor-state/Cargo.toml
    - crates/executor-state/src/lib.rs
    - crates/executor-signer/Cargo.toml
    - crates/executor-signer/src/lib.rs
    - crates/executor-mcp/Cargo.toml
    - crates/executor-mcp/src/lib.rs
    - crates/executor-mcp/src/main.rs
    - crates/executor-mcp/tests/common/mod.rs
    - crates/executor-mcp/tests/stdio_handshake.rs
  modified: []

key-decisions:
  - "Added `[lints] workspace = true` to each crate's Cargo.toml so the workspace-level clippy denylist actually propagates to every target (documented as deviation; plan text didn't call it out)."
  - "Kept executor-signer's `SignedTransaction` import live via `pub type _SignedTransactionAlias = SignedTransaction;` to prevent an unused-import warning under `workspace.lints.rust.unreachable_pub = warn`."
  - "Added `#![allow(dead_code, unreachable_pub)]` to `crates/executor-mcp/tests/common/mod.rs` so Plan 02/03 can use only a subset of the helpers without tripping `-D warnings`."
  - "Committed an empty `.gitignore` entry for `/target` (deviation Rule 3 — Cargo-managed build output should never be tracked)."

patterns-established:
  - "Stdout discipline: workspace `[workspace.lints.clippy]` + per-crate `#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]` gives two independent tripwires before runtime JSON-RPC assertions kick in."
  - "Placeholder types carry descriptive `#[schemars(description = ...)]` text that admits the placeholder nature so agents don't mistake them for complete contracts."
  - "Integration harness lives in `tests/common/mod.rs` with `env!(\"CARGO_BIN_EXE_executor-mcp\")` — downstream plans only add `#[tokio::test]` functions to `stdio_handshake.rs`."

requirements-completed: [MCP-01]

duration: 6min
completed: 2026-04-24
---

# Phase 1 Plan 01: Rust workspace + crate skeleton Summary

**4-crate Cargo workspace on 2024 edition with rmcp 1.5 deps, 7 JsonSchema tool/prompt input structs in executor-core, and a Wave 0 stdio + schema-snapshot integration harness.**

## Performance

- **Duration:** ~6 min
- **Started:** 2026-04-24T09:05:01Z
- **Completed:** 2026-04-24T09:10:35Z
- **Tasks:** 3
- **Files created:** 23
- **Files modified:** 0

## Accomplishments

- Cargo workspace bootstrapped with exactly the four Phase-1 crates (`executor-mcp`, `executor-core`, `executor-state`, `executor-signer`) — no premature `strategy-js`, `executor-evm`, or `executor-policy` crates (D-01 / D-01a).
- Stdout discipline locked in at two layers: workspace `[workspace.lints.clippy]` denies + crate-level `#![deny(...)]` in every crate's entry file (D-05).
- `executor-core` exposes 7 `JsonSchema`-derived input structs bound to the Phase-1 tool/prompt surface, plus a `SignedTransaction` placeholder that `executor-signer` imports now to lock the Phase-6 interface path.
- `executor-core` has zero dependency on `rmcp` — the core stays pure-domain so later persistence / signer / EVM crates can depend on it without pulling the MCP server.
- Wave 0 harness is in place: `spawn_server` / `send` / `recv` / `initialize` helpers plus a `harness_compiles` smoke test that already passes today, and a `schema_snapshots.rs` with 7 golden tests keyed on `tests/schemas/<Name>.json`.
- `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace --no-run` all succeed with zero warnings.

## Task Commits

1. **Task 1: Workspace root + 4 crate skeleton + stdout-discipline lint** — `937885d` (feat)
2. **Task 2: executor-core schema structs + CoreError enum** — `5ed30e9` (feat)
3. **Task 3: Wave 0 stdio + schema-snapshot harness** — `6fa5a21` (test)

## Files Created/Modified

- `Cargo.toml` — workspace root (resolver 2, edition 2024, 4 members, rmcp 1.5/schemars 1.2/tokio/tracing/serde/anyhow/thiserror/toml shared deps, `[workspace.lints.clippy]` stdout denylist).
- `rust-toolchain.toml` — pin `channel = "stable"` + rustfmt/clippy components.
- `.gitignore` — ignore `/target`.
- `crates/executor-core/src/lib.rs` — exports `error`, `schema`; workspace lints inherited; crate-level `#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]`.
- `crates/executor-core/src/error.rs` — `CoreError::InvalidInput(String)` via thiserror.
- `crates/executor-core/src/schema/mod.rs` — re-exports `strategy`, `action`, `execution`, `policy`, `prompt_args`.
- `crates/executor-core/src/schema/strategy.rs` — `StrategyRegisterInput`, `StrategyIdInput`, `StrategyRunOnceInput` with JsonSchema derive and per-field `#[schemars(description = ...)]`.
- `crates/executor-core/src/schema/action.rs` — `Action` enum skeleton (`Noop` variant, serde tagged `kind`) with a TODO(phase-4) comment listing the real variants.
- `crates/executor-core/src/schema/execution.rs` — `ExecutionIdInput` + `SignedTransaction` placeholder.
- `crates/executor-core/src/schema/policy.rs` — `PolicyUpdateInput` placeholder shape.
- `crates/executor-core/src/schema/prompt_args.rs` — `WriteEvmStrategyArgs`, `ReviewEvmStrategyArgs`.
- `crates/executor-core/tests/schema_snapshots.rs` — 7 `#[test]`s calling `schema_for!` + `assert_schema_matches_golden` with `UPDATE_SCHEMAS` env toggle.
- `crates/executor-core/tests/schemas/.gitkeep` — placeholder; Plan 02 populates JSON goldens via `UPDATE_SCHEMAS=1 cargo test -p executor-core --test schema_snapshots`.
- `crates/executor-state/src/lib.rs` — boundary doc comment only; Phase 2 adds SQLite.
- `crates/executor-signer/src/lib.rs` — empty `Signer` trait + `use executor_core::schema::execution::SignedTransaction;` (aliased to keep the import live under `unreachable_pub` lint).
- `crates/executor-mcp/src/lib.rs` — crate-level deny lints + `pub fn placeholder()`.
- `crates/executor-mcp/src/main.rs` — deny lints + no-op `fn main()` (Plan 02 wires stdio serve here).
- `crates/executor-mcp/tests/common/mod.rs` — `ServerProc`, `spawn_server`, `send`, `recv` (with JSON-RPC 2.0 assertion), `initialize` (2025-11-25 protocol version).
- `crates/executor-mcp/tests/stdio_handshake.rs` — `mod common;` + a minimal `harness_compiles` smoke test.

Plus the per-crate `Cargo.toml` files.

## Decisions Made

- **`[lints] workspace = true` in every crate's Cargo.toml.** Without this per-crate opt-in, the workspace-level `[workspace.lints.clippy]` deny list does not apply to the crate — the workspace lints system is opt-in by design. Adding it was required for the `grep "print_stdout" Cargo.toml` done-criterion to actually guard compile-time output.
- **`_SignedTransactionAlias` in executor-signer.** `workspace.lints.rust.unreachable_pub = "warn"` combined with a reachable empty trait caused no issue, but keeping a single `use` import alive avoided a follow-up unused-import warning once the trait has no methods. Documented inline so Phase 6 knows to delete the alias when real methods land.
- **`#![allow(dead_code, unreachable_pub)]` on the integration-test common module.** Plan 02 only needs a subset of `send` / `recv` / `initialize` today, and `unreachable_pub` fires because the helpers are `pub` functions in a `mod.rs` that is only reachable from its own test binary.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added `[lints] workspace = true` to every crate `Cargo.toml`**
- **Found during:** Task 1 (scaffold).
- **Issue:** Cargo's workspace lints table is opt-in per crate; without `[lints] workspace = true`, `cargo clippy -p executor-mcp` would not see the workspace-level `print_stdout = "deny"` rule, and the plan's `grep "print_stdout" Cargo.toml` done-criterion would be superficially satisfied while the runtime guard was inert.
- **Fix:** Added `[lints] workspace = true` under each crate `Cargo.toml` (`executor-mcp`, `executor-core`, `executor-state`, `executor-signer`).
- **Files modified:** `crates/executor-{mcp,core,state,signer}/Cargo.toml`.
- **Verification:** `cargo clippy --workspace --all-targets -- -D warnings` succeeds with 0 warnings; the deny list is honored.
- **Committed in:** `937885d` (Task 1 commit).

**2. [Rule 2 - Missing Critical] `.gitignore` with `/target`**
- **Found during:** Task 1 (staging for the first commit showed an untracked `target/`).
- **Issue:** Plan files list did not include `.gitignore`, but Cargo's generated `target/` would otherwise be tracked on the first `git add`.
- **Fix:** Added a minimal `.gitignore` with `/target`.
- **Files modified:** `.gitignore`.
- **Verification:** `git status` no longer shows `target/` as untracked.
- **Committed in:** `937885d` (Task 1 commit).

**3. [Rule 1 - Bug] `_SignedTransactionAlias` in `executor-signer/src/lib.rs`**
- **Found during:** Task 1 `cargo check`.
- **Issue:** The plan instructed `use executor_core::schema::execution::SignedTransaction;` inside `executor-signer/src/lib.rs`, but `Signer` trait has no methods and the import is otherwise unused. The unused-import warning would be fatal once the workspace runs with `-D warnings`.
- **Fix:** Added `#[doc(hidden)] pub type _SignedTransactionAlias = SignedTransaction;` to keep the import live without leaking it into the public API.
- **Files modified:** `crates/executor-signer/src/lib.rs`.
- **Verification:** `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- **Committed in:** `937885d` (Task 1 commit).

**4. [Rule 1 - Bug] `#![allow(dead_code, unreachable_pub)]` on `tests/common/mod.rs`**
- **Found during:** Task 3 `cargo test --workspace --no-run`.
- **Issue:** `unreachable_pub = "warn"` under `[workspace.lints.rust]` fires on every `pub fn` inside `tests/common/mod.rs` because those helpers are only reachable from the single test binary that `mod common;` includes them into. With Plan 01 only using `spawn_server`, the other helpers also trip `dead_code`. Under `-D warnings`, this would break future CI runs.
- **Fix:** Added `#![allow(dead_code, unreachable_pub)]` at the top of `tests/common/mod.rs`.
- **Files modified:** `crates/executor-mcp/tests/common/mod.rs`.
- **Verification:** `cargo clippy --workspace --all-targets -- -D warnings` clean.
- **Committed in:** `6fa5a21` (Task 3 commit).

---

**Total deviations:** 4 auto-fixed (1 blocking, 1 missing-critical, 2 bugs).
**Impact on plan:** Every fix was mechanical and required for the plan's verification gates (`-D warnings` + workspace-wide clippy propagation) to actually hold. No scope creep — no new modules, types, or features beyond what the plan specifies.

## Issues Encountered

- None. `cargo check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace --no-run` all succeed on the final tree. The `harness_compiles` integration test passes because Plan 01's `executor-mcp` binary exits cleanly on start, which `spawn_server` tolerates (it only needs the child to have been spawned, not still running).

## User Setup Required

None. All dependencies resolve from crates.io; no external services or secrets are involved.

## Next Phase Readiness

Plan 02 can:

1. `use executor_core::schema::{strategy::*, execution::*, policy::*, prompt_args::*};` directly; every type name is stable.
2. Add `#[tokio::test]` functions for `tools_list_emits_full_surface`, `unimplemented_tools_return_phase_hint`, and `readonly_tools_return_placeholder` directly to `crates/executor-mcp/tests/stdio_handshake.rs` — the `common` module is already wired in.
3. Run `UPDATE_SCHEMAS=1 cargo test -p executor-core --test schema_snapshots` once to populate `crates/executor-core/tests/schemas/*.json` golden files after the schema structs are finalized by Plan 02.
4. Wire the `rmcp::server::Server` / `#[tool_router]` / `#[prompt_router]` code into `crates/executor-mcp/src/` modules (`server.rs`, `tools.rs`, `prompts.rs`) — the lib/main entrypoints already carry the deny lints.
5. Keep `executor-core` free of `rmcp` — the serialization boundary lives in `executor-mcp`.

No blockers. No deferred items for this plan.

## Threat Flags

None beyond what the plan's `<threat_model>` already captured (T-01-01-01..04). The deviations in this plan *strengthen* T-01-01-01 mitigation (per-crate `[lints] workspace = true` makes the clippy deny list actually effective) rather than introducing new surface.

## Self-Check: PASSED

- `/Users/user/Documents/GitHub/onchain-strategy-mcp/Cargo.toml` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/rust-toolchain.toml` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/src/lib.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/src/error.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/src/schema/mod.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/src/schema/strategy.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/src/schema/action.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/src/schema/execution.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/src/schema/policy.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/src/schema/prompt_args.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/tests/schema_snapshots.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/tests/schemas/.gitkeep` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-state/src/lib.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-signer/src/lib.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/src/lib.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/src/main.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/tests/common/mod.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/tests/stdio_handshake.rs` — FOUND
- Commit `937885d` — FOUND
- Commit `5ed30e9` — FOUND
- Commit `6fa5a21` — FOUND

---
*Phase: 01-mcp-runtime-surface*
*Completed: 2026-04-24*
