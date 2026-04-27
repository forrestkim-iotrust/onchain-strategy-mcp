---
phase: 03-javascript-strategy-runner
plan: 03
status: complete
completed: 2026-04-27
subsystem: mcp-runtime
tags: [strategy_run, mcp-tool, journal-resource, error-codes, stdio-integration]
requires:
  - 03-01-SUMMARY  # Sandbox::execute, RuntimeError, D-03 limits
  - 03-02-SUMMARY  # RuntimeContext + 3 journal tables + update_run_status_with_transition
provides:
  - strategy_run-mcp-tool       # Phase-1 placeholder fully retired
  - journal-resource            # journal://{run_id} live reader
  - mcp-error-codes-32011-32017-32018
  - StrategyRunInput/Response/Outcome schemas
affects:
  - executor-mcp::tools::strategy_run
  - executor-mcp::resources::read_journal
  - executor-mcp::errors (Phase-3 codes)
  - executor-core::schema::execution (StrategyOutcome / StrategyRunResponse)
  - executor-core::schema::strategy (StrategyRunInput rename + alias)
tech-stack:
  added: []                      # No new crates — uses 03-01 strategy-js + 03-02 journal repo
  patterns:
    - 8-step strategy_run lifecycle inside spawn_blocking
    - per-call Sandbox construction (no pooling)
    - serde_json::to_value for JournalActionOutcome wire form (NEVER format!("{:?}",..))
    - structured MCP error envelope { code, message, data: { code, kind, detail, run_id } }
key-files:
  created:
    - crates/executor-core/tests/schemas/StrategyRunInput.json
    - crates/executor-core/tests/schemas/StrategyRunResponse.json
    - crates/executor-core/tests/schemas/StrategyOutcome.json
  modified:
    - crates/executor-mcp/Cargo.toml
    - crates/executor-mcp/src/errors.rs
    - crates/executor-mcp/src/server.rs
    - crates/executor-mcp/src/tools.rs
    - crates/executor-mcp/src/resources.rs
    - crates/executor-mcp/tests/stdio_handshake.rs
    - crates/executor-core/src/schema/strategy.rs
    - crates/executor-core/src/schema/execution.rs
    - crates/executor-core/tests/schema_snapshots.rs
  deleted:
    - crates/executor-core/tests/schemas/StrategyRunOnceInput.json
decisions:
  - 'Per-call Sandbox::execute construction (no pooling).'
  - 'EngineInit -> map_runtime_error("exception", "engine init: {msg}").'
  - 'Kept unimplemented_tools_return_phase_hint with one case (policy_update).'
  - 'Log-message ordering test asserts membership (HashSet), not index order.'
metrics:
  duration_min: 12
  task_count: 3
  files_created: 3
  files_modified: 9
  files_deleted: 1
  total_workspace_tests: 175
---

# Phase 3 Plan 3: strategy_run MCP tool + journal resource + STRATEGY_* error codes Summary

**Wired the assembled Phase-3 sandbox + journal infrastructure into the agent-facing `strategy_run` MCP tool, three new wire codes (-32011/-32017/-32018), the live `journal://{run_id}` resource, and a 19-test D-08a stdio integration suite — closing STR-03, STR-05, STJ-04 and Phase 3.**

## Outcomes

### Truth Statements (verified)
1. `strategy_run` is a real MCP tool: 8-step handler (validate → load → check-deleted → insert run Queued → transition Running → spawn_blocking{Sandbox::execute + RuntimeContext::flush} → validate output → record_action → transition Succeeded|Failed → respond).
2. Three new MCP error codes ship with structured `data` payloads:
   - `-32011 STRATEGY_DELETED` (`data.code = "strategy_deleted"`, `data.strategy_id`)
   - `-32017 STRATEGY_RUNTIME_ERROR` (`data.kind ∈ {"timeout","oom","stack_overflow","exception"}`, `data.run_id`)
   - `-32018 STRATEGY_INVALID_OUTPUT` (`data.detail`, `data.run_id`)
3. `map_runtime_error` dispatches every `RuntimeError` variant: `Timeout/Oom/StackOverflow/Exception` → -32017, `EngineInit` → -32017 with kind `"exception"` (prefixed `engine init:` detail), `InvalidOutput` → -32018.
4. `journal://{run_id}` returns `{run_id, source_reads:[…], actions:[…], logs:[…]}` JSON; ULID boundary check (26 alphanumeric chars) before any DB call; missing run row → `-32002 resource_not_found`.
5. Run lifecycle FSM uses `update_run_status_with_transition` for every transition (D-12); the unguarded `update_run_status` is never called from `strategy_run`.
6. Schema goldens locked: `StrategyRunInput.json` (rename of Phase-1 `StrategyRunOnceInput.json`), `StrategyRunResponse.json` (new), `StrategyOutcome.json` (new). 18 schema_snapshots tests pass.
7. 43 stdio integration tests pass (24 prior Phase-1/2 + 19 new D-08a). The Phase-1 `strategy_run_once` placeholder is fully retired in code AND tests.
8. Per-call Sandbox construction confirmed sufficient — no pooling field on `ExecutorServer`. Plan 03-01's measured `Runtime::new()` cost stays well under the 50 ms threshold.
9. Workspace: 175 tests pass, clippy clean (`-D warnings`), no `println!`/`eprintln!`/`dbg!` in `src/`.

### Acceptance Criteria
- [x] All 3 tasks committed individually (`feat(03-03)` / `feat(03-03)` / `test(03-03)`)
- [x] `cargo build --workspace` clean
- [x] `cargo test --workspace` passes — 175 tests
- [x] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [x] All 19 D-08a stdio tests use the exact verbatim names from CONTEXT D-08a
- [x] `strategy_run` returns the D-08 contract response on success
- [x] Schema goldens for input/response/outcome committed
- [x] `journal://{run_id}` reads return per-run journal payload
- [x] STR-03 / STR-05 / STJ-03 / STJ-04 marked complete in REQUIREMENTS.md
- [x] Error-code uniqueness audit vs `rmcp 1.5` registry empty
- [x] No mention of "claude" in any commit message

## Code Implementation

### Files Created
- `crates/executor-core/tests/schemas/StrategyRunInput.json` — replaces deleted `StrategyRunOnceInput.json` placeholder; identical shape, new name.
- `crates/executor-core/tests/schemas/StrategyRunResponse.json` — locks status enum, outcome tagged-enum (kind: noop|actions), required fields.
- `crates/executor-core/tests/schemas/StrategyOutcome.json` — locks the success-shape contract independently.

### Files Modified
- `crates/executor-mcp/Cargo.toml` — added `strategy-js = { path = "../strategy-js" }` per-crate dep.
- `crates/executor-mcp/src/errors.rs` — added `STRATEGY_DELETED`/`STRATEGY_RUNTIME_ERROR`/`STRATEGY_INVALID_OUTPUT` consts + `strategy_deleted`/`strategy_runtime_error`/`strategy_invalid_output` helpers + `map_runtime_error` dispatcher + 4 unit tests.
- `crates/executor-mcp/src/server.rs` — refreshed `with_instructions` blurb for Phase 3 surface (strategy_run live; journal://{run_id} live; only policy_update remains -32010).
- `crates/executor-mcp/src/tools.rs` — replaced `strategy_run_once` placeholder with the 8-step `strategy_run` handler; added file-level helpers `validate_strategy_output` / `json_type_name` / `transition` / `record_action` / `record_validation_error` / `record_runtime_error`.
- `crates/executor-mcp/src/resources.rs` — added `read_journal` async fn (boundary check → spawn_blocking{get_run + 3 list_*_for_run} → JSON body with snake_case outcome via `serde_json::to_value`); rebranched `journal://` ahead of `execution://`; updated template URI variable to `{run_id}`.
- `crates/executor-mcp/tests/stdio_handshake.rs` — added 19 D-08a tests + helper `seed_strategy`; updated `tools_list_emits_full_surface` and `unimplemented_tools_return_phase_hint` for the renamed tool; updated `resources_surface_matches_contract` for `journal://{run_id}`.
- `crates/executor-core/src/schema/strategy.rs` — renamed `StrategyRunOnceInput` → `StrategyRunInput` (with `deny_unknown_fields`); added deprecated alias `pub use StrategyRunInput as StrategyRunOnceInput`.
- `crates/executor-core/src/schema/execution.rs` — added `StrategyOutcome` (tagged enum kind=noop|actions) + `StrategyRunResponse` (run_id/strategy_id/status/started_at/finished_at/outcome).
- `crates/executor-core/tests/schema_snapshots.rs` — replaced `strategy_run_once_input_schema_stable` with three new tests: `strategy_run_input_schema_stable`, `strategy_run_response_schema_stable`, `strategy_outcome_schema_stable`.

## Key Decisions

### Sandbox construction strategy (per-call vs pooled)
**Decision:** Per-call construction inside `tokio::task::spawn_blocking`. No `runner` field on `ExecutorServer`. Plan 03-01's `Runtime::new()` cost measurement stayed well below the 50 ms decision threshold; pooling would only add lock contention without latency win, and would block parallel runs against the same `Runtime` (rquickjs `Runtime` is `!Sync` without the `parallel` feature).

### EngineInit → MCP error mapping
**Decision:** Surface `RuntimeError::EngineInit(msg)` as `STRATEGY_RUNTIME_ERROR` (-32017) with `data.kind = "exception"` and the detail prefixed `engine init: {msg}`. This keeps the agent-facing taxonomy at exactly four kinds (timeout / oom / stack_overflow / exception) — `EngineInit` is rare and host-internal; agents have no actionable distinction from a strategy-thrown exception.

### `unimplemented_tools_return_phase_hint` handling
**Decision:** Kept the test with a one-element `cases` array (`[("policy_update", 5)]`). Deleting the test was tempting since only one tool remains, but keeping the parameterised form preserves the regression-detection lattice for any future placeholder additions and matches the plan's "if kept, its updated case array" branch.

### Log-message ordering assertion
**Decision:** `strategy_run_records_log_messages` asserts the two messages are present (via `HashSet`) rather than asserting a specific index order. The journal repo orders by `(recorded_at ASC, id ASC)` and ULID `Ulid::new()` does not guarantee monotonicity within the same millisecond — both `ctx.log` calls finish inside the same RFC3339-second, so the index order is not deterministic. Auto-fix Rule 1 (bug fix in test).

## Test Coverage

### Per-crate breakdown (workspace `cargo test`):
- `executor-core` lib: 1 + `schema_snapshots` 18 = **19**
- `executor-mcp` lib: 32 (errors 9 / validation 23 incl. existing) + main 1 = **33**
- `executor-mcp` `stdio_handshake`: **43** (24 prior + 19 D-08a)
- `executor-signer` lib: **1**
- `executor-state` lib: 4 + 5 integration suites (journal_repo / partial_index_behaviour / run_base_model / run_lifecycle_transition / strategy_roundtrip) = **34**
- `strategy-js` lib: 6 + 5 integration suites (ctx_host_api / runtime_journal_flush / sandbox_entry_shape / sandbox_host_globals / sandbox_limits) = **45**

**Workspace total: 175 tests passing across 23 test suites.**

### D-08a verbatim test inventory (all 19 present)
1. `strategy_run_returns_noop_for_minimal_strategy`
2. `strategy_run_returns_actions_for_action_array_strategy`
3. `strategy_run_returns_actions_for_empty_array`
4. `strategy_run_rejects_number_return`
5. `strategy_run_rejects_object_return`
6. `strategy_run_rejects_null_return`
7. `strategy_run_rejects_promise_return`
8. `strategy_run_rejects_non_function_source`
9. `strategy_run_rejects_phase4_action_kind`
10. `strategy_run_runtime_error_on_throw`
11. `strategy_run_runtime_error_on_infinite_loop`
12. `strategy_run_runtime_error_on_oom`
13. `strategy_run_runtime_error_on_stack_overflow`
14. `strategy_run_rejects_deleted_strategy`
15. `strategy_run_records_source_read_journal_row`
16. `strategy_run_records_log_messages`
17. `strategy_run_run_row_status_transitions_to_failed_on_error`
18. `strategy_run_invalid_strategy_id_format_returns_invalid_params`
19. `strategy_run_unknown_strategy_id_returns_not_found`

### Test Resilience Notes
- Tests 11/12 (`infinite_loop` / `oom`) wrap `call_tool` in a `tokio::time::timeout(8s)` — gives ~6 s of headroom over the D-03 2-second wall-clock budget for spawn + JSON-RPC overhead.
- Tests 12/13 accept `oom|exception` and `stack_overflow|exception` — rquickjs's variant assignment depends on which budget runs out first and is not strictly classifiable.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Log ordering test assumed deterministic index order**
- **Found during:** Task 3 (test execution)
- **Issue:** `assert_eq!(logs[0].message, "hello 42"); assert_eq!(logs[1].message, "world")` failed: `Ulid::new()` does not guarantee monotonicity within the same millisecond and both `ctx.log` calls happen inside one RFC3339-second so the journal-list index order isn't stable.
- **Fix:** Switched to HashSet membership assertion. The plan-level invariant (both messages flushed) is preserved.
- **Files modified:** `crates/executor-mcp/tests/stdio_handshake.rs`
- **Commit:** f8ca233

**2. [Rule 1 - Bug] `resources_surface_matches_contract` template name drift**
- **Found during:** Task 3 (after journal:// template URI variable was renamed to `{run_id}` in Task 2)
- **Issue:** Pre-existing assertion expected `journal://{execution_id}`; Plan 03-03 §Sub-task 2.3 mandated renaming to `{run_id}` for accuracy.
- **Fix:** Updated assertion to `journal://{run_id}`.
- **Files modified:** `crates/executor-mcp/tests/stdio_handshake.rs`
- **Commit:** f8ca233

### No checkpoints, no auth gates, no architectural decisions raised.

## Verification

```bash
cargo build --workspace                                   # exit 0
cargo test --workspace                                    # 175 passed (23 suites)
cargo clippy --workspace --all-targets -- -D warnings     # 0 issues
grep -rn 'println!\|eprintln!\|dbg!' crates/*/src         # no matches
grep -r 'ErrorCode(-3201[178])' ~/.cargo/registry/src/index.crates.io-*/rmcp-1.5.0/    # empty
```

### `cargo tree` snapshot of the new dep edge
```
executor-mcp v0.1.0 → strategy-js v0.1.0 (path) → rquickjs v0.11.x
```
(verified via `cargo tree -p executor-mcp -e features --depth 2`)

## Threat Flags

None — all `<threat_model>` mitigations from the plan map to existing code paths (`record_action_outcome` precedes every Failed transition; `params!` SQL everywhere; mutex serialises run inserts; structured error envelopes do not leak storage internals).

## Phase-3 Final Test Inventory

Phase-3 added test counts (cumulative across 03-01/02/03):
- `strategy-js` lib: 6 unit (errors / limits) + 5 suites (ctx_host_api / runtime_journal_flush / sandbox_entry_shape / sandbox_host_globals / sandbox_limits) — **45 tests**
- `executor-state` journal_repo: **20 tests**
- `executor-core` schema_snapshots additions: 4 (Phase 3) — total file 18
- `executor-mcp` stdio_handshake: 19 new D-08a — total file 43
- `executor-mcp` errors mod: 4 new — total mod 9

**Plan 03-03 net adds: 19 stdio + 4 errors-mod + 3 schema-snapshots = 26 new tests.**

## Sign-off

- ROADMAP.md updated to mark Phase 3 complete (3/3 plans).
- REQUIREMENTS.md updated: STR-03, STR-05, STJ-03, STJ-04 → Complete (Phase 3 references).
- STR-04 was already Complete (closed in 03-01).
- STJ-02 already Complete (closed in 02-03).

## Self-Check: PASSED

All claimed files exist:
- `crates/executor-core/tests/schemas/StrategyRunInput.json` FOUND
- `crates/executor-core/tests/schemas/StrategyRunResponse.json` FOUND
- `crates/executor-core/tests/schemas/StrategyOutcome.json` FOUND

All claimed commits exist:
- `345a0b4` FOUND — feat(03-03): add STRATEGY_DELETED/RUNTIME_ERROR/INVALID_OUTPUT codes…
- `d686e9b` FOUND — feat(03-03): wire strategy_run MCP tool + journal://{run_id} resource…
- `f8ca233` FOUND — test(03-03): add 19 D-08a stdio integration tests…
