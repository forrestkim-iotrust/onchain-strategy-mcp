---
phase: 02-strategy-state-and-journal
plan: 02
subsystem: mcp-runtime
tags: [mcp-tools, error-mapping, validation, spawn-blocking, integration-tests]
requires:
  - executor-state::StateStore (Plan 02-01)
  - executor-core::schema::strategy response types (Plan 02-01)
  - executor-mcp::config::StateConfig (Plan 02-01)
provides:
  - executor-mcp::errors::map_state_error + STORAGE_NOT_FOUND/-32014 + STORAGE_NAME_CONFLICT/-32015 + STORAGE_ERROR/-32016 + INVALID_PARAMS/-32602
  - executor-mcp::errors::invalid_params + storage_error helpers
  - executor-mcp::validation::validate_register + validate_strategy_id_format + MAX_SOURCE_BYTES/MAX_NAME_CHARS/MAX_DESCRIPTION_CHARS/MAX_TAGS/MAX_TAG_CHARS
  - executor-mcp::ExecutorServer { state: Arc<tokio::sync::Mutex<StateStore>> } via fallible new(&StateConfig)
  - 5 storage-backed MCP tool bodies (strategy_register/list/get/delete + execution_get)
  - strategy://{id} resource read returning live StrategyGetResponse JSON
  - common::spawn_server_with_state + call_tool + extract_json_result test helpers
affects:
  - executor-mcp config now strictly required at boot (no Default for ExecutorServer)
  - Phase 1 tests narrowed: only strategy_run_once + policy_update return -32010; only policy_get keeps placeholder shape
tech-stack:
  added:
    - tempfile 3 (executor-mcp dev-dep — needed for per-test config tempfile + restart-persistence tempdir)
  patterns:
    - "Async DB bridge: tokio::task::spawn_blocking + state.blocking_lock() (RESEARCH Pattern 2). Tokio mutex never held across await."
    - "Validation in two places: schema-level (advisory) + handler-side (enforced) per D-09b."
    - "Per-test isolation: each #[tokio::test] spawns its own binary with :memory: state via spawn_server_with_state."
    - "Resource boundary id check mirrors validation::validate_strategy_id_format but surfaces as resource_not_found (-32002), not invalid_params (-32602), per the resources/read contract."
key-files:
  created:
    - crates/executor-mcp/src/validation.rs
  modified:
    - crates/executor-mcp/src/errors.rs
    - crates/executor-mcp/src/lib.rs
    - crates/executor-mcp/src/server.rs
    - crates/executor-mcp/src/main.rs
    - crates/executor-mcp/src/tools.rs
    - crates/executor-mcp/src/resources.rs
    - crates/executor-mcp/Cargo.toml
    - crates/executor-mcp/tests/common/mod.rs
    - crates/executor-mcp/tests/stdio_handshake.rs
    - .gitignore
decisions:
  - "Default for ExecutorServer + no-arg new() removed. Phase 2 cannot fall back to a defaulted state store because StateStore::open is fallible; every caller (main + integration tests via spawn_server_with_state) must provide a StateConfig."
  - "ReadResourceResult constructor: rmcp 1.5 marks the struct #[non_exhaustive], so the plan's `ReadResourceResult { contents }` literal failed to compile. Switched to ReadResourceResult::new(vec![..]); semantics identical."
  - "ResourceContents::text(text, uri).with_mime_type(\"application/json\") — the ::TextResourceContents { uri, mime_type, text, meta } enum variant exists but the helper constructor pre-fills meta=None and the builder lets us override mime_type after the fact. Equivalent to the plan's intent."
  - "Plan 1+2 collapsed into one commit by design (plan deviation note): server.rs's new state field forces tools.rs/resources.rs to update in lockstep; splitting would have left the tree red between commits."
  - "Resource-boundary malformed-id surfaces as resource_not_found (-32002) with data.code=\"malformed_id\", NOT as invalid_params (-32602). Reason: resources/read is its own MCP method with a typed not_found path; bleeding -32602 into resource reads would conflate validation errors with resource lookup errors."
  - "common::spawn_server_with_state persists the temp config file via tmp.into_temp_path().keep() so the child can read it after the parent's tempfile guard would normally drop. Restart test uses tempfile::tempdir() (NOT NamedTempFile) so SQLite WAL+shm sidecars stay co-located and clean up on dir drop."
metrics:
  duration_seconds: 480
  duration_human: "~8 minutes"
  tasks_total: 3
  tasks_completed: 3
  files_created: 1
  files_modified: 10
  tests_added: 30
  workspace_tests_passing: 82
  completed_date: "2026-04-27"
---

# Phase 02 Plan 02: Strategy MCP Tool Surface Summary

Five MCP tools (`strategy_register` / `_list` / `_get` / `_delete` / `execution_get`) plus the `strategy://{id}` resource transition from Phase 1 placeholders to real `StateStore`-backed behaviour. New typed error mapping (`map_state_error` + three storage codes), handler-side D-09 validation, and 14 stdio integration tests covering the full CONTEXT D-08a contract — including soft-delete idempotency, name reuse after delete, and restart persistence.

## What Was Built

### `executor-mcp::errors` — typed storage error mapping

Extended (not rewritten) the existing `unimplemented_err` module with:

- `STORAGE_NOT_FOUND = ErrorCode(-32014)` — strategy / run miss
- `STORAGE_NAME_CONFLICT = ErrorCode(-32015)` — `strategy_register` active-name collision with different source
- `STORAGE_ERROR = ErrorCode(-32016)` — wraps any `StateError::Storage(_)` plus non-`StateError` failures (`spawn_blocking` join, JSON serialise) so the storage-path data envelope stays uniform (`data.code == "storage_error"`)
- `INVALID_PARAMS = ErrorCode(-32602)` — JSON-RPC standard, used for D-09 validation failures

`map_state_error(StateError) -> McpError` is the single dispatch site agents bind against. The name-conflict branch builds the canonical message `"strategy name 'arb' already used by strategy_id=… (created …); soft-delete that strategy to reuse the name, or choose a different name"` plus structured `data` (`attempted_name`, `existing_strategy_id`, `existing_created_at`). 4 new unit tests pin every branch.

### `executor-mcp::validation` — handler-side D-09 enforcement (new module)

Schema-level `maxLength` constraints are advisory (serde does not enforce them); the tool entrypoint re-checks every bound and names the violated constraint in the message (D-09b). Constants `MAX_SOURCE_BYTES = 256 * 1024`, `MAX_NAME_CHARS = 128`, `MAX_DESCRIPTION_CHARS = 4096`, `MAX_TAGS = 16`, `MAX_TAG_CHARS = 64`.

`validate_register` checks:
- source byte length (NOT char count — Pitfall 8) and emptiness
- name char count + whitespace-only rejection
- description char count
- tags array length, per-tag char count, per-tag whitespace-only rejection

`validate_strategy_id_format` enforces `^[0-9a-f]{64}$` for `strategy_delete` (D-09a). 12 unit tests cover every boundary.

### `executor-mcp::server` — state wiring

`ExecutorServer` now carries `state: Arc<tokio::sync::Mutex<StateStore>>`. Phase 1's no-arg `new()` and `Default` impl are **removed** — opening SQLite is fallible, so the constructor is `new(&StateConfig) -> anyhow::Result<Self>`. The `#[tool_handler] #[prompt_handler]` block stays on a single `impl ServerHandler` (Pitfall 6); only `read_resource` changed signature internally to forward `self.state.clone()` to `resources::read_resource_impl`.

### `executor-mcp::tools` — 5 tool transitions

Every transitioned tool follows the same pattern:

```text
validate(input) -> spawn_blocking { state.blocking_lock(); store.<call>(...) }
                  -> map_state_error -> shape response -> json_result
```

- `strategy_register` → `RegisterOutcome::Created` / `AlreadyExists` mapped to `StrategyRegisterResponse`. Idempotent path preserves the existing row's name (response carries both top-level `name` and `existing_name` set to the same value, plus `existing_description` / `existing_tags`).
- `strategy_list` → `StrategyListResponse` with `StrategyListItem` (no `source` field — D-07a / T-02-02-03 mitigation).
- `strategy_get` → matches `StrategyGetInput::ById { strategy_id }` / `ByName { name }` and dispatches to the appropriate store method. None → `map_state_error(StateError::NotFound("strategy"))` so wire shape stays consistent.
- `strategy_delete` → `validate_strategy_id_format` first (D-09a, T-02-02-02), then `soft_delete_strategy`. Idempotent (the underlying repo returns the original `deleted_at` on repeat).
- `execution_get` → real DB lookup; returns `ExecutionGetResponse` if a run exists, otherwise `not_found`. No runs are inserted in Phase 2, so this path effectively always returns not_found until Plan 02-03 wires `strategy_run_once`.

`strategy_run_once` and `policy_update` continue returning `unimplemented_err(-32010)`. `policy_get` keeps its placeholder shape.

### `executor-mcp::resources` — strategy://{id} live read

`read_resource_impl` now takes `Arc<tokio::sync::Mutex<StateStore>>` as a third arg. Branches:

- `strategy://{id}` → boundary check (`64 lowercase-hex` — T-02-02-05) → `spawn_blocking` `get_strategy_by_id` → serialize `StrategyGetResponse` as the text body of a `ResourceContents::text(body, uri).with_mime_type("application/json")`. Malformed id → `resource_not_found(-32002)` with `data.code = "malformed_id"`.
- `execution://*` / `journal://*` → structured `resource_not_found` with `data.phase = 6` / `3` envelope so agents can distinguish "phase-gated" from "real miss".
- Anything else → `resource_not_found` with `data.phase = 2`.

### `executor-mcp::tests::common` + `stdio_handshake.rs`

`spawn_server_with_state(db_path)` writes `[state]\npath = "..."\n` to a `NamedTempFile`, persists it via `.into_temp_path().keep()` (so the child can read it after the parent's auto-delete guard would normally drop), and exports `EXECUTOR_CONFIG` to the spawned binary. `call_tool` + `extract_json_result` cut boilerplate.

14 new `#[tokio::test]` fns:

| Name                                                  | Coverage                                                 |
| ----------------------------------------------------- | -------------------------------------------------------- |
| `strategy_register_creates_row`                       | Happy-path INSERT; id is 64 hex; created_at populated.   |
| `strategy_register_idempotent_same_source`            | Same source, different name → existing row's name wins.  |
| `strategy_register_conflict_same_name_different_source` | -32015 with attempted_name + existing_strategy_id.     |
| `strategy_register_rejects_oversized_source`          | 262145 bytes → -32602; message names actual + limit.     |
| `strategy_register_rejects_empty_name`                | "   " → -32602; message includes "whitespace-only".      |
| `strategy_list_excludes_source_payload`               | Two rows, no `source` field on any list item.            |
| `strategy_list_filters_deleted_by_default`            | Default 1 item; include_deleted=true → 2 items.          |
| `strategy_get_by_id_returns_source`                   | Round-trip preserves source bytes.                       |
| `strategy_get_by_name_only_returns_active`            | Soft-deleted "arb" by name → -32014 not_found.           |
| `strategy_delete_is_soft_and_idempotent`              | Repeat delete returns same deleted_at; id still findable. |
| `soft_deleted_name_can_be_reused`                     | Delete then re-register same name new source → new id.   |
| `resource_read_strategy_uri_returns_body`             | mimeType=application/json; text deserialises with source. |
| `execution_get_returns_not_found_when_empty`          | -32014 not_found from fresh DB.                          |
| `strategies_persist_across_restart`                   | Two spawns against `tempfile::tempdir()/state.db`; second sees the row. |

Phase 1 regression tests narrowed in lockstep:

- `unimplemented_tools_return_phase_hint`: `cases` shrunk to `[("strategy_run_once", 6), ("policy_update", 5)]`. Spawns with `:memory:` state so the binary boots.
- `readonly_tools_return_placeholder` → renamed `policy_get_returns_placeholder` and slimmed to the one tool that still returns a placeholder.
- `resources_surface_matches_contract`: the `data.phase == 1` assertion was replaced with `data.code == "malformed_id"` because `strategy://nonexistent` no longer hits the Phase-1 catch-all — it falls into the new resource-boundary id check.

## Verification

```text
cargo build -p executor-mcp                                    # clean
cargo test -p executor-mcp --lib errors::                     # 5 passed (1 existing + 4 new)
cargo test -p executor-mcp --lib validation::                  # 12 passed
cargo test -p executor-mcp --test stdio_handshake              # 22 passed (8 baseline → 22 with 14 new + 2 narrowed)
cargo test --workspace                                         # 82 passed across 14 suites
cargo clippy --workspace --all-targets -- -D warnings          # clean
```

`grep -rn 'println!\|eprintln!\|dbg!' crates/executor-mcp/src/` → no matches.

## Commits

| Tasks  | Hash      | Summary                                                                                       |
| ------ | --------- | --------------------------------------------------------------------------------------------- |
| 1 + 2  | `3ee27fa` | wire StateStore into MCP tools + add error mapping & validation                              |
| 3      | `989ebe3` | add 14 stdio integration tests for Phase 2 strategy contract                                  |

Tasks 1 and 2 were combined per the plan's explicit deviation note (server.rs's new `state` field forces tools.rs / resources.rs to update in the same commit, otherwise the tree is red between commits). Task 3 is a clean follow-on commit.

## Deviations from Plan

The plan was executed end-to-end. Three minor compile-time / semantic adjustments worth surfacing — none of these change agent-facing contract:

1. **Task 1+2 commit collapse** — explicitly authorised by the plan's `<action>` deviation note. Recorded for traceability.
2. **`ReadResourceResult` is `#[non_exhaustive]` on rmcp 1.5.** The plan's `ReadResourceResult { contents }` struct literal failed to compile (E0639). Switched to `ReadResourceResult::new(vec![..])` — semantics identical. Captured in `decisions:` frontmatter.
3. **Borrow ordering in `read_resource_impl`.** The plan's snippet `read_strategy(uri, id.to_string(), state)` failed E0505 because `id` borrowed `uri` while `uri` was being moved. Fixed by binding `let id_owned = id.to_string();` before the call. Trivial; no semantic change.

One additional housekeeping fix logged separately:

4. **`.gitignore` extended** for `state.db` / `state.db-shm` / `state.db-wal`. The MCP binary's `[state].path` defaults to `./state.db`, so building/running the binary in a worktree (which `cargo test` does indirectly) leaks SQLite WAL artefacts into `git status`. Treated as a Rule 3 fix — would have polluted future commits.

## Authentication Gates

None — Phase 2 Wave 2 stays fully offline (stdio MCP + local SQLite).

## Requirements Closed

- **STR-01** — agent registers a JS strategy with name + source + description + tags; idempotent on same source; name-unique-among-active enforced (`strategy_register_idempotent_same_source`, `strategy_register_conflict_same_name_different_source`).
- **STR-02** — list, inspect, soft-delete, and reuse names after delete (`strategy_list_*`, `strategy_get_*`, `strategy_delete_is_soft_and_idempotent`, `soft_deleted_name_can_be_reused`).
- **STJ-01** — strategies persist across server restart (`strategies_persist_across_restart` exercises two binary spawns against the same on-disk DB and proves the row survives).

## Threat Mitigations Verified

| Threat ID  | Mitigation                                                                  | Test                                                           |
| ---------- | --------------------------------------------------------------------------- | -------------------------------------------------------------- |
| T-02-02-01 | `validate_register` 256 KiB byte cap                                        | `strategy_register_rejects_oversized_source`                   |
| T-02-02-02 | `validate_strategy_id_format` `^[0-9a-f]{64}$` pre-DB                       | implicit (any malformed id → invalid_params before SQL)        |
| T-02-02-03 | `StrategyListItem` has no `source` field                                    | `strategy_list_excludes_source_payload`                        |
| T-02-02-04 | All DB calls in `spawn_blocking`                                            | implicit; `grep -c spawn_blocking tools.rs resources.rs` = 6   |
| T-02-02-05 | Hex-only id check in `read_strategy` before any FS/DB access                | `resources_surface_matches_contract` (malformed_id branch)     |
| T-02-02-06 | `StrategyGetInput` `#[serde(untagged, deny_unknown_fields)]`                | inherited from Plan 02-01 schema golden                        |
| T-02-02-07 | D-01b idempotence with `already_exists: true`                               | `strategy_register_idempotent_same_source`                     |
| T-02-02-09 | `map_state_error` emits structured `data.code` + safe human message         | `map_state_error_*` unit tests                                 |
| T-02-02-10 | Each test spawns its own process with `:memory:` or unique `tempdir()`      | structural — `spawn_server_with_state` enforces                |

## rmcp 1.5 Adaptation Notes

- `ResourceContents::text(text, uri).with_mime_type("application/json")` is the canonical builder; the variant constructor `ResourceContents::TextResourceContents { uri, mime_type: ..., text, meta }` is also available but the helper pre-fills `meta = None` and lets us override `mime_type` fluently.
- `ReadResourceResult` is `#[non_exhaustive]` → must use `::new(contents: Vec<ResourceContents>)`.
- `ListResourcesResult` and `ListResourceTemplatesResult` accept struct-literal initialisation (already used by Phase 1).

## Error-Code Collision Check

`grep -r 'ErrorCode(-3201[456]' ~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-1.5.0/` returns no hits — `-32014` / `-32015` / `-32016` are free in the rmcp internal model. `-32602` is the JSON-RPC 2.0 reserved standard code (intentional reuse, agents already recognise it).

## tempfile dev-dep

Plan 02-01 added `tempfile = "3"` only to `crates/executor-state/Cargo.toml`. This plan adds it to `crates/executor-mcp/Cargo.toml` `[dev-dependencies]` (mirroring the per-crate-only precedent — Plan 02-01 explicitly chose not to promote new deps to workspace level).

## Known Stubs

None. All five transitioned tools call the live `StateStore` backend; the resource read returns real serialised rows; placeholders that remain (`strategy_run_once`, `policy_update`, `policy_get`) are explicitly phase-gated and tested as such. `execution_get` returns `not_found` only because no run-insertion path exists yet — this is the contract until Plan 02-03 wires `strategy_run_once`.

## Self-Check: PASSED

- `crates/executor-mcp/src/validation.rs` exists on disk.
- 10 modified files (errors.rs, lib.rs, server.rs, main.rs, tools.rs, resources.rs, Cargo.toml, common/mod.rs, stdio_handshake.rs, .gitignore) reflect intended changes.
- Both commit hashes (`3ee27fa`, `989ebe3`) present in `git log`.
- 82 workspace tests pass (52 baseline + 30 net new — 17 unit + 13 stdio additions including 2 narrowed Phase 1 tests retained).
- Clippy clean across the workspace.
