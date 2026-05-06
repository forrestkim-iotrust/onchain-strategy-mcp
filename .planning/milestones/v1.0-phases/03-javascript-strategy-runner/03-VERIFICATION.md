---
phase: 03-javascript-strategy-runner
verified: 2026-04-27T00:00:00Z
status: passed
score: 4/4 success criteria + 5/5 requirements verified
overrides_applied: 0
re_verification: null
---

# Phase 3: JavaScript Strategy Runner — Verification Report

**Phase Goal:** Runtime executes sandboxed JavaScript strategies and accepts only valid `Action[]`/noop outputs.
**Verified:** 2026-04-27
**Status:** passed (PASS-WITH-NOTES)
**Re-verification:** No — initial verification.

---

## Goal Achievement

### Observable Truths (ROADMAP Success Criteria)

| #   | Truth                                                                  | Status     | Evidence |
| --- | ---------------------------------------------------------------------- | ---------- | -------- |
| 1   | Agent can run a registered JS strategy once                            | ✓ VERIFIED | `strategy_run` tool registered (`tools.rs` `name = "strategy_run"`); 8-step handler executes Sandbox::execute and journals; `strategy_run_returns_noop_for_minimal_strategy`, `_returns_actions_for_action_array_strategy`, `_returns_actions_for_empty_array` pass via stdio |
| 2   | Forbidden host access is blocked                                       | ✓ VERIFIED | `Context::builder().with::<intrinsic::All>` (no module/import/require/loader); `FORBIDDEN_GLOBALS_SCRUB` JS prelude deletes 14 globals incl. `child_process`, `fs`, `Deno`, `process`, `Worker`, `fetch`, `setTimeout`, `setInterval`, `setImmediate`, `queueMicrotask`, `console`, `XMLHttpRequest`, `WebSocket`; `sandbox_host_globals` 8 tests + `cargo tree -p strategy-js` audit (no libloading, no tokio via rquickjs) |
| 3   | Source reads and returned actions/errors are journaled                 | ✓ VERIFIED | 3 tables exist (`journal_source_reads`, `journal_actions`, `journal_logs`) per D-06; `RuntimeContext::flush` writes source-read marker + drains log buffer in single mutex acquisition; `record_action_outcome` writes per-outcome row; stdio `_records_source_read_journal_row`, `_records_log_messages`, `_run_row_status_transitions_to_failed_on_error` all pass |
| 4   | Invalid return shapes are rejected with actionable MCP tool errors     | ✓ VERIFIED | `STRATEGY_INVALID_OUTPUT (-32018)` wired in `errors.rs`; `validate_strategy_output` covers number/object/null/non-function/promise/phase4-action-kind; structured `data: {code, detail, run_id}`; 6 stdio rejection tests pass |

**Score:** 4/4 success criteria verified.

### Required Artifacts

| Artifact                                                                | Expected                                                | Status     | Details |
| ----------------------------------------------------------------------- | ------------------------------------------------------- | ---------- | ------- |
| `crates/strategy-js/`                                                   | New crate (D-02)                                        | ✓ VERIFIED | Crate exists with `Cargo.toml`, `src/{lib,error,limits,sandbox,runtime}.rs` |
| `crates/strategy-js/src/limits.rs`                                      | D-03 constants 2s/64MiB/8MiB/1MiB                       | ✓ VERIFIED | `WALL_CLOCK_MS=2_000`, `MEMORY_LIMIT_BYTES=64*1024*1024`, `GC_THRESHOLD_BYTES=8*1024*1024`, `MAX_STACK_BYTES=1024*1024`; const-time non-zero + GC<heap asserts; unit test pins exact values |
| `crates/strategy-js/src/sandbox.rs`                                     | D-05 Shape B + D-10 promise reject                      | ✓ VERIFIED | IIFE wrap: `(() => { const __fn = (SOURCE); if (typeof __fn !== 'function') return '__STRATEGY_NOT_FUNCTION__'; return __fn(__ctx); })()`; promise detection → `InvalidOutput(detail mentions "promise")` |
| `crates/strategy-js/src/runtime.rs`                                     | RuntimeContext (D-04 ctx host)                          | ✓ VERIFIED | Implements CtxHost; `default_clock`, `flush()` idempotent single-mutex |
| `crates/executor-state/src/journal.rs`                                  | Journal repo for 3 tables                               | ✓ VERIFIED | `record_source_read`, `record_action_outcome`, `record_log`, `list_*_for_run`; `phase3_emittable` gate before INSERT |
| `crates/executor-state/src/schema.rs`                                   | 3 new CREATE TABLEs (D-06)                              | ✓ VERIFIED | `journal_source_reads`, `journal_actions`, `journal_logs` all present (idempotent) |
| `crates/executor-state/src/runs.rs::update_run_status_with_transition`  | D-12 strict transition guard (closes 02-REVIEW MR-01)   | ✓ VERIFIED | Atomic `UPDATE WHERE id=? AND status=?from`; explicit terminal-state guard rejects `Succeeded → *` BEFORE SQL; row re-query distinguishes NotFound vs InvalidInput |
| `crates/executor-mcp/src/errors.rs`                                     | -32011 / -32017 / -32018 codes (D-07)                   | ✓ VERIFIED | Three `pub const` codes match exact values; helpers `strategy_deleted`, `strategy_runtime_error`, `strategy_invalid_output`, dispatcher `map_runtime_error` |
| `crates/executor-mcp/src/tools.rs::strategy_run`                        | Real tool replacing Phase-1 placeholder (D-08)          | ✓ VERIFIED | `name = "strategy_run"`; 8-step handler (validate → load → check-deleted → insert Queued → transition Running → spawn_blocking{Sandbox + flush} → record_action → transition terminal) |
| `crates/executor-mcp/src/resources.rs::read_journal`                    | journal://{run_id} live reader                          | ✓ VERIFIED | ULID boundary check → spawn_blocking{get_run + 3 list_*_for_run} → JSON `{run_id, source_reads, actions, logs}`; missing run → -32002 |
| `crates/executor-core/tests/schemas/StrategyRunInput.json`              | Renamed schema golden                                   | ✓ VERIFIED | Present; `StrategyRunOnceInput.json` deleted |
| `crates/executor-core/tests/schemas/StrategyRunResponse.json`           | New schema golden                                       | ✓ VERIFIED | Present (3.1K, locks status enum + tagged outcome) |
| `crates/executor-core/tests/schemas/StrategyOutcome.json`               | New schema golden                                       | ✓ VERIFIED | Present (1KB, kind: noop\|actions tagged) |
| `crates/executor-core/tests/schemas/JournalActionOutcome.json`          | Future-locked enum (6 variants)                         | ✓ VERIFIED | Present; locks `noop/actions/validation_error/runtime_error/simulation_failure/policy_denied` |
| `crates/executor-mcp/tests/stdio_handshake.rs`                          | 19 D-08a stdio tests with verbatim names                | ✓ VERIFIED | `grep -E 'async fn strategy_run_'` returns 19 — all verbatim names from CONTEXT D-08a present |

### Key Link Verification

| From                            | To                                          | Via                                                              | Status   | Details |
| ------------------------------- | ------------------------------------------- | ---------------------------------------------------------------- | -------- | ------- |
| `executor-mcp::tools::strategy_run` | `strategy_js::Sandbox::execute`        | `tokio::task::spawn_blocking` w/ `RuntimeContext` capture        | ✓ WIRED  | `Cargo.toml` declares `strategy-js = { path = "../strategy-js" }`; tools.rs imports + invokes; tests prove end-to-end |
| `RuntimeContext::flush`         | `journal_source_reads` + `journal_logs`     | Single `state.blocking_lock()` → `record_source_read` + drain logs | ✓ WIRED  | `runtime_context_flush_writes_source_read_marker`, `_buffers_logs_during_execute_then_flush_writes_them`, `_is_idempotent` pass |
| `strategy_run` handler          | `update_run_status_with_transition`         | Queued → Running → Succeeded\|Failed via guarded API only        | ✓ WIRED  | All 3 transitions in handler use guarded variant; unguarded `update_run_status` not called |
| `errors::map_runtime_error`     | All 6 `RuntimeError` variants               | Match arms → -32017 (kind) or -32018 (detail)                    | ✓ WIRED  | Unit tests cover Timeout/Oom/StackOverflow/Exception/EngineInit/InvalidOutput dispatch |
| `journal://` resource template  | `read_journal` reader                       | resource registered in resources.rs with `{run_id}` placeholder  | ✓ WIRED  | `resources_surface_matches_contract` stdio test asserts new template |

### Data-Flow Trace (Level 4)

| Artifact                       | Data Source                                                                 | Produces Real Data | Status     |
| ------------------------------ | --------------------------------------------------------------------------- | ------------------ | ---------- |
| `strategy_run` response        | `Sandbox::execute` JSON → `validate_strategy_output` → real DB run row      | Yes                | ✓ FLOWING  |
| `journal://{run_id}` body      | 3 `list_*_for_run` queries against rusqlite `Connection`                    | Yes                | ✓ FLOWING  |
| `ctx.now()` inside JS          | `chrono::Utc::now().timestamp_millis()` snapshot at run start (D-04)        | Yes (clock-bound)  | ✓ FLOWING  |
| `ctx.log` → journal_logs       | `Rc<RefCell<Vec<String>>>` host buffer → drained to `record_log` post-exec  | Yes                | ✓ FLOWING  |

No HOLLOW / DISCONNECTED / STATIC artifacts identified.

### Behavioral Spot-Checks

| Behavior                                          | Command                                                       | Result                          | Status |
| ------------------------------------------------- | ------------------------------------------------------------- | ------------------------------- | ------ |
| Workspace builds clean with all phases            | `cargo build --workspace`                                     | exit 0                          | ✓ PASS |
| Workspace tests all pass                          | `cargo test --workspace`                                      | 175 passed across 23 suites     | ✓ PASS |
| Clippy clean with `-D warnings`                   | `cargo clippy --workspace --all-targets -- -D warnings`       | 0 issues                        | ✓ PASS |
| 19 D-08a stdio tests with verbatim `strategy_run_*` names | `grep -E 'async fn strategy_run_' .../stdio_handshake.rs \| wc -l` | 19                       | ✓ PASS |

### Requirements Coverage

| Requirement | Source Plan(s)            | Description                                                                                            | Status       | Evidence |
| ----------- | ------------------------- | ------------------------------------------------------------------------------------------------------ | ------------ | -------- |
| STR-03      | 03-02 / 03-03             | Agent runs registered strategy via `strategy_run`; ctx surface observable; STJ-03 source-read row     | ✓ SATISFIED  | strategy_run handler + RuntimeContext flush + ctx_host_api 14 tests + `strategy_run_records_source_read_journal_row` |
| STR-04      | 03-01                     | Strategy code cannot access keys / fs / process / network / RPC                                       | ✓ SATISFIED  | sandbox_host_globals 8 tests + cargo-tree audit (no libloading, no tokio); FORBIDDEN_GLOBALS_SCRUB + Context::builder w/o module intrinsics |
| STR-05      | 03-01 / 03-03             | Runtime rejects unsupported return shapes                                                              | ✓ SATISFIED  | D-05 Shape B sentinel + D-10 promise reject + 6 stdio rejection tests + validate_strategy_output |
| STJ-03      | 03-02 / 03-03             | Runtime records source reads per run                                                                   | ✓ SATISFIED  | journal_source_reads schema + record_source_read repo + RuntimeContext::flush marker + stdio test |
| STJ-04      | 03-02 / 03-03             | Runtime records returned actions and validation errors                                                 | ✓ SATISFIED  | journal_actions schema + JournalActionOutcome with phase3_emittable + handler records on every Failed/Succeeded path |

No orphaned requirements. No phase-3-mapped REQUIREMENTS.md ID is missing from a plan's `requirements:` field.

### Anti-Patterns Found

| File                                              | Line | Pattern                                                                  | Severity | Impact |
| ------------------------------------------------- | ---- | ------------------------------------------------------------------------ | -------- | ------ |
| `crates/executor-mcp/src/tools.rs`                | 6-7  | Stale `//!` module doc claims `strategy_run_once` is a placeholder       | ℹ️ Info  | Cosmetic doc drift only — comment is wrong but unused; tool registration is correct (`name = "strategy_run"`). Recommend cleanup in next phase. |
| `crates/executor-mcp/src/errors.rs`               | 4    | `//!` mentions `strategy_run_once` (legacy)                              | ℹ️ Info  | Same — doc string lag. |
| `crates/executor-core/src/schema/execution.rs`    | 10-11 | `ExecutionIdInput` schemars description references `strategy_run_once`   | ℹ️ Info  | Customer-visible JSON-schema description mentions retired tool name; not blocking — `execution_get` semantics unchanged. Cleanup recommended. |
| `crates/executor-core/tests/schemas/ExecutionIdInput.json` | 8 | Mirror of above — golden contains stale name                  | ℹ️ Info  | Cosmetic; the input shape itself is correct. |

No 🛑 Blocker or ⚠️ Warning patterns. No TODO/FIXME/placeholder/console.log/empty-handler stubs in Phase-3 production paths. No `unimplemented_err(-32010)` hits for `strategy_run` (only `policy_update` retains the placeholder, per CONTEXT design).

### Human Verification Required

None. Phase 3 is a closed-loop runtime layer with no UI / external-service / human-only checks. Every behaviour is observable via `cargo test --workspace` and was verified.

### Phase 4 Readiness

The following Phase-4 prerequisites are stable APIs and compile-locked by goldens / tests:

- ✓ `strategy_js::Sandbox::execute(source, &mut CtxHost)` — signature locked
- ✓ `strategy_js::CtxHost` trait — extension surface for `ctx.evm.*` (CTX-01..09)
- ✓ `strategy_js::RuntimeContext` — Phase-4 will add EVM cache + `ctx.evm` injection without changing flush contract
- ✓ `executor_state::StateStore::{record_source_read, record_action_outcome, record_log, update_run_status_with_transition}` — repo APIs ship
- ✓ `executor_core::schema::execution::{StrategyOutcome, StrategyRunResponse}` — schema goldens locked; Phase 4 extends `StrategyOutcome::Actions { actions: Vec<Action> }` by adding new `Action` variants only

Phase 4 is unblocked.

### Gaps Summary

No gaps blocking goal achievement. The four Info-level doc-drift items (`strategy_run_once` mentions in comments / `ExecutionIdInput` schemars description) are leftover documentation lag, not behavioral defects — the deprecated `pub use StrategyRunInput as StrategyRunOnceInput` alias in `executor-core::schema::strategy` is intentional per CONTEXT D-08 (one-phase deprecation window). All 4 ROADMAP success criteria, all 5 requirements (STR-03/04/05, STJ-03/04), all 12 locked decisions (D-01..D-12 incl. D-08a), and the 02-REVIEW MR-01 closure are verified in code with passing tests.

**Verdict:** PASS-WITH-NOTES. The phase goal is achieved; the four documentation-drift Info items are cosmetic and recommended for cleanup early in Phase 4.

---

*Verified: 2026-04-27*
*Verifier: Claude (gsd-verifier)*
