---
phase: 02-strategy-state-and-journal
plan: 01
subsystem: storage
tags: [sqlite, rusqlite, content-addressed, soft-delete, schema-goldens]
requires:
  - executor-core::schema (Phase 1)
  - executor-mcp::config (Phase 1 [logging] precedent)
provides:
  - executor-state::StateStore (open/register/list/get/soft_delete + run CRUD)
  - executor-core::schema::execution::RunStatus (7 variants, phase2_emittable gate)
  - executor-core::schema::strategy response types (Register/List/Get/Delete/StrategyGetInput)
  - executor-core::schema::execution::ExecutionGetResponse
  - executor-mcp::config::StateConfig with default ./state.db
affects:
  - executor-mcp config loader (StateConfig field added; --config=PATH parsing fixed)
tech-stack:
  added:
    - rusqlite 0.39 (bundled)
    - sha2 0.11
    - hex 0.4
    - ulid 1.2
    - chrono 0.4
    - tempfile 3 (dev-only)
  patterns:
    - "PRAGMA before DDL for FK enforcement (D-03c)"
    - "Partial unique index for soft-deleted name reuse (D-01c)"
    - "Content-addressed register: 3-step decision tree (existing-id → existing-name conflict → insert)"
    - "Free-function repository + thin StateStore façade (no trait abstraction at v1)"
    - "Future-reserved enum variants locked in goldens at first introduction (D-05)"
key-files:
  created:
    - crates/executor-state/src/error.rs
    - crates/executor-state/src/schema.rs
    - crates/executor-state/src/store.rs
    - crates/executor-state/src/strategies.rs
    - crates/executor-state/src/runs.rs
    - crates/executor-state/tests/common/mod.rs
    - crates/executor-state/tests/partial_index_behaviour.rs
    - crates/executor-state/tests/strategy_roundtrip.rs
    - crates/executor-state/tests/run_base_model.rs
    - crates/executor-core/tests/schemas/RunStatus.json
    - crates/executor-core/tests/schemas/StrategyGetInput.json
    - crates/executor-core/tests/schemas/StrategyRegisterResponse.json
    - crates/executor-core/tests/schemas/StrategyListResponse.json
    - crates/executor-core/tests/schemas/StrategyGetResponse.json
    - crates/executor-core/tests/schemas/StrategyDeleteResponse.json
    - crates/executor-core/tests/schemas/ExecutionGetResponse.json
  modified:
    - crates/executor-state/Cargo.toml
    - crates/executor-state/src/lib.rs
    - crates/executor-core/src/schema/strategy.rs
    - crates/executor-core/src/schema/execution.rs
    - crates/executor-core/tests/schema_snapshots.rs
    - crates/executor-core/tests/schemas/StrategyRegisterInput.json (regenerated; metadata→description+tags)
    - crates/executor-core/tests/schemas/StrategyIdInput.json (description text refresh)
    - crates/executor-mcp/src/config.rs
    - config.example.toml
decisions:
  - "Mutex placement: StateStore holds bare Connection; outer Arc<tokio::sync::Mutex<StateStore>> lives in executor-mcp (Plan 02-02). Avoids nested locking; matches RESEARCH Pattern 2."
  - "RunStatus: all 7 variants declared at Phase 2 to lock the schema golden once (D-05)."
  - "phase2_emittable() lives on executor-core::RunStatus (Phase 2 gate point), invoked from executor-state::runs (boundary check)."
  - "tags column: JSON-array TEXT; on read, decode failure silently maps to None — DB is single-writer so corruption is not expected."
  - "soft_delete idempotency: returns existing deleted_at unchanged on repeat call (D-07c) — avoids agent confusion about timestamp drift."
  - "auto-fill finished_at when status transitions to Succeeded/Failed (terminal); other states leave it untouched."
  - "Workspace deps NOT promoted: rusqlite/sha2/hex/ulid/chrono/tempfile pinned per-crate, mirroring Phase 1 [logging]-only precedent."
metrics:
  duration_seconds: 393
  duration_human: "~6.5 minutes"
  tasks_total: 3
  tasks_completed: 3
  files_created: 16
  files_modified: 9
  tests_added: 22
  workspace_tests_passing: 52
  completed_date: "2026-04-27"
---

# Phase 02 Plan 01: Strategy State Foundation Summary

Local SQLite persistence layer for strategies and run base-model: `executor-state` crate with content-addressed strategy registration, soft-delete semantics, ULID-keyed runs, and the full agent-facing response schema set in `executor-core` — all with no MCP wiring yet (lands in Plan 02-02).

## What Was Built

### `executor-state` crate (greenfield)

**`schema.rs`** — `open_conn(path)` opens a connection, applies pragmas (`journal_mode = WAL`, `synchronous = NORMAL`, `foreign_keys = ON`) **before** any DDL, then runs `CREATE TABLE IF NOT EXISTS` for `strategies` and `runs` plus three indexes:

- `idx_strategies_name_active` — unique partial index on `(name) WHERE deleted_at IS NULL`, enabling name reuse after soft-delete (D-01c).
- `idx_strategies_deleted_at` — supports the default `WHERE deleted_at IS NULL` filter on `list`.
- `idx_runs_strategy_id` — supports `list_runs_for_strategy` (Phase 3+ surface).

Schema is fully idempotent — second open against the same file preserves data and does not error.

**`error.rs`** — `StateError` with four variants (`Storage` / `NotFound` / `NameConflict { fields }` / `InvalidInput`) + `From<rusqlite::Error>`. MCP error-code mapping is deferred to Plan 02-02.

**`store.rs`** — `StateStore` owns a single `rusqlite::Connection` (no inner mutex). Façade methods delegate to module-level free functions in `strategies` / `runs`. The `#[doc(hidden)] __test_conn()` accessor exposes raw SQL for the partial-index integration tests without leaking to production callers.

**`strategies.rs`** — content-addressed CRUD. `hash_source()` returns `hex(sha256(source))` matching FIPS 180-4 vectors. `register()` follows the three-step decision tree from RESEARCH Pattern 3:

1. If a row with `id == hash_source(source)` already exists → `RegisterOutcome::AlreadyExists(existing_row)` (idempotent, returns the original row's name/description/tags untouched).
2. Else if a non-deleted row with the requested name exists → `StateError::NameConflict { existing_strategy_id, existing_source_hash, existing_created_at, attempted_name }`.
3. Else INSERT.

`list(include_deleted)` projects an explicit column set (no `SELECT *`) so `source` is **never** copied into list responses (T-02-01-03 mitigation). `get_by_id()` returns rows regardless of deletion state; `get_by_name()` filters to active rows only. `soft_delete()` is idempotent — a second call returns the original `deleted_at` unchanged.

**`runs.rs`** — ULID-keyed run base CRUD. `insert_run` / `update_run_status` reject future-reserved statuses (`Canceled`, `SimulationDenied`, `PolicyDenied`) at the boundary by consulting `RunStatus::phase2_emittable()` and emitting `StateError::InvalidInput` with a "reserved for Phase 5/6" message. Terminal statuses (`Succeeded` / `Failed`) auto-fill `finished_at`; other transitions leave it untouched. FK violations bubble through as `StateError::Storage` so the `foreign_keys_enforced` test can regression-guard the `PRAGMA foreign_keys = ON` invariant (Pitfall 1).

### `executor-core` schema additions

- **`StrategyRegisterInput` split**: `metadata: Option<Value>` removed → top-level `description: Option<String>` + `tags: Option<Vec<String>>` (RESEARCH Open Q4 option B; D-07a). Golden regenerated.
- **`StrategyGetInput`**: `#[serde(untagged, deny_unknown_fields)]` XOR enum of `ById { strategy_id }` / `ByName { name }`.
- **`RunStatus`**: 7 snake-case variants (`queued` / `running` / `succeeded` / `failed` / `canceled` / `simulation_denied` / `policy_denied`) with `phase2_emittable()` returning `true` only for the first four. The schema golden enumerates all 7 — Phase 5/6 will not trigger contract churn.
- **Response types** (Phase 2 base): `StrategyRegisterResponse`, `StrategyListItem` + `StrategyListResponse` (sourceless), `StrategyGetResponse` (source-bearing), `StrategyDeleteResponse`, `ExecutionGetResponse`. All derive the standard `Debug + Clone + Serialize + Deserialize + JsonSchema` set.
- **`schema_snapshots.rs`**: 7 new `#[test]` fns lock the new types; existing `StrategyRegisterInput` golden was regenerated (intentional diff).

### `executor-mcp` config extension

- `Config { logging, state }` with `StateConfig::default()` → `path = "./state.db"` (D-03e: omitted `[state]` section still loads).
- `parse_cli_config_path()` testable helper accepts both `--config PATH` and `--config=PATH` forms — closes `01-REVIEW` issue **IN-01** where the equals form was silently dropped.
- `deny_unknown_fields` on both `Config` and `StateConfig`; existing top-level unknown-field test was rewritten to use `[policy]` (since `[state]` is now valid).
- `config.example.toml` documents the new section.

## Verification

```text
cargo test --workspace          # 52 tests passing across 14 suites
cargo test -p executor-state    # 18 tests (5 partial-index + 10 strategy + 3 runs)
cargo test -p executor-core --test schema_snapshots   # 14 schema goldens
cargo test -p executor-mcp --lib config::tests        # 11 config tests
cargo clippy --workspace --all-targets -- -D warnings # clean
```

All commands exit 0. No `println!` / `eprintln!` / `dbg!` permitted (workspace clippy denylist + per-crate `#![deny]` tripwires both clean).

## Commits

| Task | Hash      | Summary                                                                 |
| ---- | --------- | ----------------------------------------------------------------------- |
| 1    | `9201af1` | scaffold executor-state crate with schema + StateStore + StateError     |
| 2    | `a7f6d00` | strategies + runs CRUD + executor-core response schemas                 |
| 3    | `6b54f12` | add [state] config section + fix --config=PATH parser (IN-01)           |

## Deviations from Plan

None. Plan executed exactly as written across all 3 tasks. All acceptance criteria met:

- Task 1: 5 partial-index tests pass; clippy clean; all `grep` invariants present.
- Task 2: 18 executor-state tests + 14 schema-snapshot tests pass; goldens enumerate all 7 RunStatus variants; `metadata` removed from `StrategyRegisterInput.json`, `description` + `tags` present.
- Task 3: 11 config tests pass; both CLI forms parse via the extracted helper; `[state]` documented in `config.example.toml`.

Two minor adjustments worth noting (not deviations — discretion items the plan explicitly delegated):

1. **`runs.rs` Task 1 stub** declared `Run.status: String` to keep Task 1 self-contained; Task 2 immediately replaced this with `RunStatus`. This kept the Task 1 commit buildable without depending on Task 2's `executor-core` changes — strict task atomicity.
2. **`StrategyIdInput.json` golden refreshed** alongside the regenerated `StrategyRegisterInput.json` because I tightened its description text to mention the hex SHA-256 format (forward-link to D-09a regex Plan 02-02 will add). Diff is descriptive only — `properties` / `required` shape unchanged.

## Authentication Gates

None — Phase 2 Wave 1 is fully offline (SQLite + schemars only).

## Requirements Closed

- **STR-01** — agent registers a JS strategy with name + source + (now) description + tags. Repository contract tested; MCP wiring lands in Plan 02-02.
- **STJ-01** — strategies persist locally in `strategies` table; soft-delete preserves journal/run integrity.
- **STJ-02** — runs persist with `id` (ULID) + `strategy_id` (FK) + `started_at` + `status`; CRUD round-trip tested; full Phase 3 wiring (status transitions emitted from JS execution path) deferred.

## Known Stubs

None. No UI surfaces, hardcoded empty data flows, or "coming soon" placeholders introduced in this plan. The MCP tool handlers for `strategy_register`/`strategy_list`/`strategy_get`/`strategy_delete`/`execution_get` are still Phase 1 placeholders returning `-32010 unimplemented` — that is **the contract** for Wave 1 of this Phase, intentional and tracked: Plan 02-02 wires the storage layer through. Not a stub by the verifier definition.

## Self-Check: PASSED

- All 16 created files present on disk.
- All 9 modified files reflect intended changes.
- 3 commit hashes (`9201af1`, `a7f6d00`, `6b54f12`) present in `git log`.
- 52 workspace tests pass.
- Clippy clean across executor-state, executor-core, executor-mcp.
