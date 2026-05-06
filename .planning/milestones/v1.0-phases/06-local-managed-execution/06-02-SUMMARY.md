---
phase: 06-local-managed-execution
plan: 02
subsystem: execution
tags: [rust, alloy, signer, sqlite, local-managed-execution]

requires:
  - phase: 06-local-managed-execution
    provides: [executor-signer local signer boundary and non-secret signer config]
provides:
  - execution_actions SQLite persistence for broadcast, receipt, gas, and error rows
  - Alloy wallet-provider broadcast and receipt waiting APIs in executor-signer
  - strategy_run handoff that executes approved actions sequentially after policy and simulation pass
affects: [phase-06-local-managed-execution, executor-state, executor-signer, executor-mcp]

tech-stack:
  added: [reqwest direct dependency in executor-signer for URL parsing]
  patterns:
    - execution_actions rows keyed by run_id and original action_index
    - split broadcast-before-receipt signer API
    - short spawn_blocking database writes around async network waits

key-files:
  created:
    - crates/executor-state/src/executions.rs
    - crates/executor-state/tests/execution_actions.rs
    - crates/executor-signer/tests/local_execution.rs
    - crates/executor-mcp/tests/execution_actions.rs
  modified:
    - Cargo.lock
    - crates/executor-state/src/schema.rs
    - crates/executor-state/src/store.rs
    - crates/executor-state/src/lib.rs
    - crates/executor-signer/Cargo.toml
    - crates/executor-signer/src/lib.rs
    - crates/executor-signer/src/local.rs
    - crates/executor-signer/src/error.rs
    - crates/executor-mcp/src/tools.rs

key-decisions:
  - "execution_actions uses UNIQUE(run_id, action_index) and deterministic upsert semantics for broadcast rows."
  - "LocalSignerHandle exposes split broadcast and wait_for_receipt APIs so tx hash can be persisted before receipt waiting."
  - "strategy_run loads signer config at execution boundary without editing ExecutorServer constructors."

patterns-established:
  - "Persist tx hash immediately after Provider::send_transaction returns, before waiting for receipt."
  - "Map signer/RPC failure details to stable execution error kinds before persistence or MCP response."

requirements-completed: [EXE-08, EXE-09, STJ-06]

duration: 15 min
completed: 2026-04-28
---

# Phase 06 Plan 02: Local Managed Execution Summary

**Receipt-backed sequential local managed execution with per-action SQLite audit rows and Alloy wallet-provider broadcast**

## Performance

- **Duration:** 15 min
- **Started:** 2026-04-28T16:05:42Z
- **Completed:** 2026-04-28T16:20:43Z
- **Tasks:** 3/3
- **Files modified:** 13

## Accomplishments

- Added `execution_actions` schema and repository methods for broadcast, receipt, gas, and stable execution error persistence.
- Implemented `LocalSignerHandle::broadcast` and `LocalSignerHandle::wait_for_receipt` with Alloy wallet-enabled providers, tx-hash-first pending execution, and stable receipt status strings.
- Inserted `execute_approved_actions` after Phase 5 policy/simulation gates and before success journaling so non-noop approved actions execute sequentially in original action-index order.
- Kept noop-only runs independent of signer config and preserved the plan constraint to avoid `ExecutorServer` constructor edits.

## Task Commits

Each task was committed atomically:

1. **Task 1 RED: Add failing execution action repository tests** - `a53e724` (test)
2. **Task 1 GREEN: Add execution action persistence** - `ea047de` (feat)
3. **Task 2 RED: Add failing local execution signer tests** - `f051455` (test)
4. **Task 2 GREEN: Add local signer broadcast API** - `0e4187a` (feat)
5. **Task 3: Insert sequential execution loop into strategy_run after Phase 5 gates** - `4584760` (feat)

**Plan metadata:** committed separately after this summary.

## Files Created/Modified

- `Cargo.lock` - Locks direct `reqwest` dependency additions for signer URL parsing.
- `crates/executor-state/src/schema.rs` - Adds `execution_actions` table and run-id index.
- `crates/executor-state/src/executions.rs` - Implements execution row repository and public entry structs.
- `crates/executor-state/src/store.rs` - Adds `StateStore` execution façade methods.
- `crates/executor-state/src/lib.rs` - Exports execution row types.
- `crates/executor-state/tests/execution_actions.rs` - Covers roundtrip, ordering, and unique `(run_id, action_index)` behavior.
- `crates/executor-signer/Cargo.toml` - Adds direct `reqwest` dependency for URL parsing type compatibility with Alloy HTTP provider.
- `crates/executor-signer/src/error.rs` - Adds stable broadcast/receipt errors and execution error kind mapping.
- `crates/executor-signer/src/lib.rs` - Re-exports local execution structs and receipt status.
- `crates/executor-signer/src/local.rs` - Adds wallet-provider broadcast, pending execution, receipt wait, and receipt status mapping.
- `crates/executor-signer/tests/local_execution.rs` - Covers stable strings, error taxonomy, and broadcast API error redaction.
- `crates/executor-mcp/src/tools.rs` - Adds execution-loop insertion after simulation gates and helper DB writes.
- `crates/executor-mcp/tests/execution_actions.rs` - Covers missing-signer fail-closed execution row behavior.

## Decisions Made

- Used deterministic upsert for duplicate broadcast rows rather than allowing duplicate action rows or nondeterministic constraint failures.
- Added direct `reqwest = "0.13"` in `executor-signer` to parse URLs with the same type Alloy HTTP provider expects.
- Loaded signer config via `crate::config::load()?.signer_config()?` inside `strategy_run`, preserving the revised no-constructor-edits constraint.

## GitNexus Impact Analysis

- `StateStore` struct and impl upstream impact: LOW, no indexed direct callers or affected processes.
- `strategy_run` upstream impact: LOW, no indexed upstream dependants.
- `record_action` upstream impact: LOW, one direct caller (`strategy_run`) and one affected process.
- `transition` upstream impact: LOW, one direct caller (`strategy_run`) and one affected process.
- `LocalSignerHandle`, `SignerError`, and new `execute_approved_actions` were not found in the stale GitNexus index before edits.
- `gitnexus detect-changes` was run before task commits and reported no indexed changes because the index is stale/read-only in this worktree.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added direct reqwest dependency for Alloy HTTP URL parsing**
- **Found during:** Task 2 (Implement Alloy wallet broadcast and receipt wait)
- **Issue:** `ProviderBuilder::connect_http` expects a `reqwest::Url`; the signer crate did not directly depend on `reqwest`.
- **Fix:** Added `reqwest = "0.13"` to `crates/executor-signer/Cargo.toml` and used it for URL parsing.
- **Files modified:** `crates/executor-signer/Cargo.toml`, `Cargo.lock`, `crates/executor-signer/src/local.rs`
- **Verification:** `cargo test -p executor-signer broadcast && cargo test -p executor-signer wait_for_receipt && cargo test -p executor-signer local_signer` passed.
- **Committed in:** `0e4187a`

**2. [Rule 2 - Missing Critical] Added a targeted missing-signer execution row test**
- **Found during:** Task 3 (Insert sequential execution loop)
- **Issue:** The planned `cargo test -p executor-mcp execution_actions` filter had no matching executor-mcp test yet, so execution-row behavior at the MCP boundary was not provable.
- **Fix:** Added `crates/executor-mcp/tests/execution_actions.rs` covering `signer_not_configured` failure persistence and stable MCP error data.
- **Files modified:** `crates/executor-mcp/tests/execution_actions.rs`
- **Verification:** `cargo test -p executor-mcp signer_not_configured && cargo test -p executor-mcp execution_actions` passed.
- **Committed in:** `4584760`

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 missing critical)
**Impact on plan:** Both fixes were necessary to compile against Alloy's HTTP provider and to make the planned MCP execution-row acceptance command meaningful. No architectural changes or constructor edits were introduced.

## Issues Encountered

- GitNexus index writes failed with read-only FTS warnings and the index is stale at `b5aa3f0`; impact analysis still ran where symbols were indexed, and direct tests/grep checks scoped changes.
- The anvil feature command `cargo test -p executor-mcp --features anvil-tests --test stdio_handshake local_managed_execution -- --nocapture` completed successfully but matched zero tests because this plan did not add a `local_managed_execution` stdio test name.

## Known Stubs

- `crates/executor-mcp/src/tools.rs` module header still mentions pre-existing placeholder text for unrelated `strategy_run_once` / `policy_get` phase notes. This is historical commentary, not a new execution-loop stub and does not block this plan's objective.

## Threat Flags

None beyond the plan threat model. This plan intentionally adds the signer-loop-to-RPC and RPC-receipt-to-state trust boundaries described in T-06-02-01 through T-06-02-05.

## Verification

- `cargo test -p executor-state execution_actions` — passed, 3 matching tests.
- `cargo test -p executor-signer` — passed, 9 tests across unit/integration/doc suites.
- `cargo test -p executor-mcp signer_not_configured` — passed, 1 matching test.
- `cargo test -p executor-mcp execution_actions` — passed, 1 matching test.
- `cargo test -p executor-mcp --features anvil-tests --test stdio_handshake local_managed_execution -- --nocapture` — command succeeded, 0 matching tests.
- `cargo clippy -p executor-state -p executor-signer -p executor-mcp --all-targets -- -D warnings` — passed with no issues.

## TDD Gate Compliance

- RED commits present: `a53e724` for Task 1 and `f051455` for Task 2.
- GREEN commits present after RED: `ea047de` for Task 1 and `0e4187a` for Task 2.
- Task 3 was marked TDD in the plan but was implemented as a single feature commit with tests included, not a separate RED then GREEN sequence. This is a gate compliance warning for Task 3.

## User Setup Required

External local signer configuration is required before non-noop managed execution can broadcast transactions:

- Set `[signer].private_key_env = "EXECUTOR_PRIVATE_KEY"` in runtime config.
- Set `EXECUTOR_PRIVATE_KEY` in the local operator environment to a hex EVM private key for the configured RPC/chain.
- Keep raw private keys out of committed config, logs, and strategy JavaScript.

## Next Phase Readiness

Plan 06-03 can build the `execution_get` and `execution://{run_id}` status/report surfaces from persisted `execution_actions` rows. Broadcast, receipt, gas, error kind/detail, signer address, and action-index ordering are now available in state.

## Self-Check: PASSED

- Found `.planning/phases/06-local-managed-execution/06-02-SUMMARY.md`.
- Found `crates/executor-state/src/executions.rs`.
- Found `crates/executor-state/tests/execution_actions.rs`.
- Found `crates/executor-signer/tests/local_execution.rs`.
- Found `crates/executor-mcp/tests/execution_actions.rs`.
- Found task commit `a53e724`.
- Found task commit `ea047de`.
- Found task commit `f051455`.
- Found task commit `0e4187a`.
- Found task commit `4584760`.

---
*Phase: 06-local-managed-execution*
*Completed: 2026-04-28*
