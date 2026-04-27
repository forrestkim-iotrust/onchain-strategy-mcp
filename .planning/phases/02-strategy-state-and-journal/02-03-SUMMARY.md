---
phase: 02-strategy-state-and-journal
plan: 03
subsystem: executor-state + executor-mcp
type: execute
wave: 3
tags: [persistence, runs, lifecycle, mcp-stdio, schema-contract, phase2-close]
requirements:
  closes: [STJ-02]
  advances: [STR-01, STR-02, STJ-01]
dependency_graph:
  requires:
    - "02-01: executor-state crate, RunStatus enum + phase2_emittable, RunRepo CRUD"
    - "02-02: execution_get tool wired to StateStore over Arc<Mutex<>> + spawn_blocking"
  provides:
    - "Run lifecycle contract: started_at on insert, finished_at exactly on Succeeded/Failed"
    - "Stable list_runs_for_strategy ordering (started_at ASC, id ASC)"
    - "ULID Crockford shape contract on run_id"
    - "Phase2 emittability gate on BOTH insert_run and update_run_status"
    - "End-to-end stdio proof: insert via StateStore, observe via execution_get"
    - "Locked 7-variant RunStatus golden via run_status_schema_includes_future_variants (D-08a)"
  affects:
    - "Future Phase 3 strategy_run_once: can rely on update_run_status NotFound + finished_at semantics"
    - "Future Phase 5/6: cannot silently rename Canceled / SimulationDenied / PolicyDenied wire names"
tech-stack:
  added: []
  patterns:
    - "Out-of-band DB seeding for stdio integration tests (open StateStore directly, drop, then spawn server)"
    - "JSON Schema walker that collects both `enum[]` strings and `const` strings (schemars 1.x oneOf shape)"
    - "Test-only StateStore helper `__test_insert_run_with_time` (#[doc(hidden)] pub) — Option A from plan"
key-files:
  created: []
  modified:
    - crates/executor-state/src/runs.rs
    - crates/executor-state/src/store.rs
    - crates/executor-state/tests/run_base_model.rs
    - crates/executor-mcp/tests/stdio_handshake.rs
decisions:
  - "Adopted Option A (test-only helper) over Option B (sleep-based ordering) — sleep would have added ≥2s flake-prone sleep to the suite for no semantic gain. Helper is `#[doc(hidden)]` and uses `__test_` prefix to mark intent."
  - "list_runs_for_strategy switched from `ORDER BY started_at DESC` (Plan 02-01 default) to `ORDER BY started_at ASC, id ASC` per Plan 02-03 acceptance — DESC was a vestigial choice; ASC + id tie-breaker matches the documented contract (D-04b ordering, Pitfall 6 same-second collision)."
  - "RunStatus future-variants walker collects strings from BOTH `enum` arrays AND `const` fields. The schemars 1.x emission for the RunStatus enum is `oneOf: [{enum:[queued,running,succeeded,failed]}, {const:canceled}, {const:simulation_denied}, {const:policy_denied}]` — a single `enum[]` of 7 would not match the actual golden. The plan's collect_enums walker (enum-only) would have failed; this is a Rule 1 fix documented here."
  - "update_run_status reserved-variant gate was already present from Plan 02-01 — no runs.rs production-code edit was needed for emittability. Only `list_runs_for_strategy` ORDER BY clause and the test-only helper were added."
metrics:
  duration_minutes: ~5
  tasks: 2
  files_modified: 4
  files_created: 0
  commits: 2
  tests_added: 10
  tests_total_phase2: 60  # state(10 strategy + 11 run + 5 partial-index = 26) + core(14) + mcp(24 stdio) - subtract baseline duplicates
  completed: 2026-04-27
---

# Phase 2 Plan 03: Strategy State & Journal — Lifecycle Contract & End-to-End Proof Summary

Closed STJ-02 with end-to-end verification: a run inserted directly via `StateStore::insert_run` (out-of-band) is observable through MCP `execution_get` over stdio, and its lifecycle (Queued → Running → Succeeded) is persisted across server restarts with `finished_at` populated only on the terminal transition. Locked the 7-variant `RunStatus` wire schema as a regression-proof contract.

## What Changed

### Run repository (`crates/executor-state/src/runs.rs`, `store.rs`)
- `list_runs_for_strategy` SQL: `ORDER BY started_at DESC` → `ORDER BY started_at ASC, id ASC`. The `id` tie-breaker handles same-second `now_rfc3339` (seconds-granularity per Plan 02-01) collisions deterministically.
- Added `runs::insert_run_with_started_at(conn, strategy_id, status, started_at)` — `pub(crate)` test-only seam, gated on `phase2_emittable`, identical to `insert_run` except the timestamp is caller-supplied.
- Added `StateStore::__test_insert_run_with_time(strategy_id, status, started_at)` — `#[doc(hidden)] pub` façade so integration tests in the **tests/** crate (a separate compilation unit) can plant deterministic timestamps. Production callers MUST use `insert_run`; the `__test_` prefix and doc-hidden attribute mark intent.

No production-code semantic changes — Plan 02-01 already shipped the `phase2_emittable` gate on both `insert_run` and `update_run_status`, and `update_run_status` already returns `StateError::NotFound` on `affected == 0` and sets `finished_at` via `COALESCE` only when status is `Succeeded`/`Failed`. This plan locked all of that with tests.

### Run repository tests (`crates/executor-state/tests/run_base_model.rs`)

Added 8 tests (3 from Plan 02-01 → 11 total):

| Test | Asserts |
|------|---------|
| `update_run_status_sets_finished_at_on_succeeded` | finished_at None until Succeeded transition |
| `update_run_status_sets_finished_at_on_failed` | finished_at populated on Failed |
| `update_run_status_leaves_finished_at_none_on_queued_or_running` | finished_at stays None for non-terminal |
| `update_run_status_on_missing_id_returns_not_found` | `StateError::NotFound(_)` includes the run id |
| `insert_run_returns_ulid_shape` | 26 chars, ASCII alphanumeric, uppercase, no I/L/O/U |
| `list_runs_for_strategy_orders_by_started_at_asc` | 3-row ASC ordering with planted timestamps |
| `list_runs_for_strategy_excludes_other_strategy_runs` | Cross-strategy isolation |
| `update_run_status_rejects_reserved_variant` | InvalidInput("reserved") + DB row unchanged |

### MCP stdio tests (`crates/executor-mcp/tests/stdio_handshake.rs`)

Added 2 tests (22 from Plan 02-02 → 24 total):

- **`run_roundtrip_insert_get_update_status`** (D-08a #11) — full end-to-end:
  1. `tempdir().join("state.db")`; open `StateStore` directly, register strategy + insert_run(Queued); drop.
  2. Spawn server → `execution_get` → assert status="queued", started_at non-empty, finished_at null.
  3. Drop server; reopen StateStore; `update_run_status(Running)`; drop.
  4. Spawn server → `execution_get` → assert status="running", finished_at still null.
  5. Drop server; `update_run_status(Succeeded)`; drop.
  6. Spawn server → `execution_get` → assert status="succeeded", finished_at populated.

- **`run_status_schema_includes_future_variants`** (D-08a #12) — schema golden walker:
  - Reads `../executor-core/tests/schemas/RunStatus.json`.
  - Walks the value tree collecting strings from **both** `enum[]` arrays AND `const` fields into a `BTreeSet<String>`.
  - Asserts all 7 expected variants present: `queued`, `running`, `succeeded`, `failed`, `canceled`, `simulation_denied`, `policy_denied`.

## Why this matters

STJ-02 in REQUIREMENTS.md required runs to have a durable `run_id` (ULID), `strategy_id` (FK), `started_at` (RFC3339), and `status` (enum). After Plan 02-02 the `execution_get` tool was wired but had no data — it could only prove the not-found path. This plan proves the happy path through real persistence: a row written before the MCP server starts is retrievable through the agent surface, and the wire enum cannot regress without an explicit, intentional golden update.

## Self-Check: PASSED

Acceptance criteria evidence:

```
$ cargo test -p executor-state --test run_base_model
cargo test: 11 passed (1 suite, 0.01s)

$ cargo test -p executor-mcp --test stdio_handshake -- run_roundtrip_insert_get_update_status run_status_schema_includes_future_variants
cargo test: 2 passed, 22 filtered out (1 suite, 0.29s)

$ cargo test --workspace
cargo test: 92 passed (14 suites, 0.30s)

$ cargo clippy --workspace --all-targets -- -D warnings
cargo clippy: No issues found
```

Grep-level acceptance:
- `grep -c "ORDER BY started_at" crates/executor-state/src/runs.rs` → `1` ≥ 1 ✓
- `grep -c "phase2_emittable" crates/executor-state/src/runs.rs` → `3` ≥ 2 (insert_run, insert_run_with_started_at, update_run_status) ✓
- `grep -c "__test_insert_run_with_time" crates/executor-state/src/store.rs` → `1` ≥ 1 ✓
- 8 new test names present in run_base_model.rs ✓
- `run_roundtrip_insert_get_update_status` and `run_status_schema_includes_future_variants` each appear once in stdio_handshake.rs ✓
- `policy_denied`, `simulation_denied`, `canceled` each appear in the future-variants assertion list ✓

Commits exist:
- `449660c` task 1 (run repo lifecycle / ordering / shape) ✓
- `c5fa5fb` task 2 (stdio roundtrip + RunStatus future-variants) ✓

## Phase 2 Test Inventory (final)

| Crate / file | Tests | Notes |
|---|---|---|
| `executor-core::tests/schema_snapshots.rs` | 14 | Golden JSON Schema for every contract type — locked Plan 02-01 |
| `executor-state::tests/run_base_model.rs` | 11 | 3 from 02-01, 8 from 02-03 |
| `executor-state::tests/strategy_roundtrip.rs` | 10 | Plan 02-01 |
| `executor-state::tests/partial_index_behaviour.rs` | 5 | Plan 02-01 (FK + partial unique idx) |
| `executor-mcp::tests/stdio_handshake.rs` | 24 | 8 from 01-01..01-03, 14 from 02-02, 2 from 02-03 |
| **Total Phase 1+2** | **64 across 5 files** | + 28 doctests/unit tests = **92 workspace** |

## Pitfalls Encountered

- **Schema golden walker shape** (Rule 1 fix vs plan): The plan's `collect_enums` walker only inspected `enum[]` arrays. The actual `RunStatus.json` schemars 1.x emission uses a `oneOf` with one `{enum: [4 variants]}` plus three `{const: "..."}` siblings. A pure-enum walker would have failed the test. Final implementation collects strings from BOTH `enum[]` and `const` keys into a `BTreeSet`, then checks set membership for all 7 expected names. Documented in decisions.
- **Tempfile + WAL leftover artifacts** (Pitfall 5 from RESEARCH): Not encountered. Each spawn-server-then-drop cycle in `run_roundtrip_insert_get_update_status` cleanly drops both the `StateStore` (close WAL on Drop) and the child process (kill_on_drop=true). Test passes deterministically across 5 runs.
- **Same-second `now_rfc3339` collisions**: Avoided entirely by Option A (test helper with caller-supplied timestamp). The plan's recommendation matched the lowest-flake choice.

## Phase 2 Completion Sign-Off

Phase 2 (strategy-state-and-journal) is complete:
- 02-01: storage layer + schemas + `[state]` config — `executor-state` crate landed.
- 02-02: 4 strategy tools + `execution_get` wired through `Arc<Mutex<StateStore>>` + spawn_blocking; resources/read for `strategy://{id}`.
- 02-03: run lifecycle contract proved end-to-end; future-variant wire schema locked.

`02-VALIDATION.md` can flip `nyquist_compliant: true` — every Wave 0 helper exists (`fresh_memory_store`, `seed_strategies`, `spawn_server_with_state`, `call_tool`, `extract_json_result`); every task has a grep-able automated verify; no manual-only verifications remain.

Phase 3 (`strategy_run_once` + sandboxed JS) can begin without risk of schema-drift or run-status contract regression.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `collect_enums` walker spec did not match `RunStatus.json` shape**
- **Found during:** Task 2 implementation
- **Issue:** The plan's pseudo-code only collected strings under `enum` arrays. The schemars 1.x golden uses `oneOf: [{enum:[...]}, {const:...}, {const:...}, {const:...}]` so a pure-enum walker would only find 4 variants and the assertion would always fail.
- **Fix:** Walker now collects strings from both `enum` arrays and `const` fields into a `BTreeSet<String>`, then checks set membership for all 7 expected names.
- **Files modified:** `crates/executor-mcp/tests/stdio_handshake.rs`
- **Commit:** `c5fa5fb`

**2. [Rule 1 - Bug] Wave 1 left `list_runs_for_strategy` with `ORDER BY started_at DESC`, contradicting D-04b ordering decision**
- **Found during:** Task 1 sub-task 1.2
- **Issue:** Plan 02-01 shipped `ORDER BY started_at DESC` (vestigial from an earlier draft). D-04b and Plan 02-03 acceptance specify ASC, with `id` as a tie-breaker.
- **Fix:** Changed SQL to `ORDER BY started_at ASC, id ASC`.
- **Files modified:** `crates/executor-state/src/runs.rs`
- **Commit:** `449660c`

No checkpoints triggered. No Rule 4 architectural decisions needed. No auth gates. Plan executed end-to-end autonomously.
