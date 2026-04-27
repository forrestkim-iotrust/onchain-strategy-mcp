---
phase: 02-strategy-state-and-journal
reviewed: 2026-04-27T00:00:00Z
depth: standard
files_reviewed: 20
files_reviewed_list:
  - crates/executor-state/src/lib.rs
  - crates/executor-state/src/error.rs
  - crates/executor-state/src/schema.rs
  - crates/executor-state/src/store.rs
  - crates/executor-state/src/strategies.rs
  - crates/executor-state/src/runs.rs
  - crates/executor-state/Cargo.toml
  - crates/executor-core/src/schema/strategy.rs
  - crates/executor-core/src/schema/execution.rs
  - crates/executor-mcp/src/config.rs
  - crates/executor-mcp/src/errors.rs
  - crates/executor-mcp/src/server.rs
  - crates/executor-mcp/src/tools.rs
  - crates/executor-mcp/src/validation.rs
  - crates/executor-mcp/src/resources.rs
  - crates/executor-mcp/src/lib.rs
  - crates/executor-mcp/src/main.rs
  - crates/executor-mcp/Cargo.toml
  - config.example.toml
findings:
  critical: 0
  high: 0
  medium: 2
  low: 4
  info: 4
  total: 10
status: issues_found
---

# Phase 2: Code Review Report

**Reviewed:** 2026-04-27
**Depth:** standard
**Files Reviewed:** 20 source files (test files excluded per scope)
**Status:** issues_found (none blocking; 2 medium quality issues + 4 low + 4 info)

## Summary

Phase 2 code is in solid shape. SQL is uniformly parameterized (no injection vectors found), pragmas are applied before DDL as designed, the `Mutex<Connection>` + `spawn_blocking` async bridge is consistently used, and validation is enforced both in schema and at handler entrypoints per D-09. Error mapping to MCP codes (`-32014/-32015/-32016/-32602`) matches the D-08a contract documented in 02-CONTEXT exactly.

No CRITICAL or HIGH findings. Two MEDIUM items concern (a) `update_run_status` allowing post-terminal transitions that leak stale `finished_at`, and (b) `map_state_error` echoing raw SQLite error text in `data.detail` (low impact for a local-trust MCP, but unnecessary surface). LOW/INFO items are mostly documentation/clarity nits — misleading field naming in `NameConflict`, an unnecessary clone, an implicit FK action, and a non-informative `not_found` `data.resource` for `strategy_get`.

No security vulnerabilities found. No `unwrap()`/`expect()` on user-input paths. No hardcoded secrets. No `unsafe`. Public API surface is appropriately scoped — the two `__test_*` accessors are `#[doc(hidden)]` and prefix-marked.

## Medium

### MR-01: `update_run_status` permits non-monotonic lifecycle transitions; stale `finished_at` after un-terminating

**File:** `crates/executor-state/src/runs.rs:105-125`
**Issue:** `update_run_status` accepts any phase2-emittable status without checking the current row's status. Consequences:

1. Once a run reaches `Succeeded` or `Failed` (terminal), `finished_at` is set. A subsequent `update_run_status(_, Running)` succeeds — `finished_at` is NOT cleared (the `COALESCE(?2, finished_at)` clause only writes when the new status is also terminal). The row then carries `status="running"` with a populated `finished_at`, an inconsistent state.
2. There is no guard preventing illegal transitions (e.g., `Succeeded → Queued`).

Phase 2 has no production caller for `update_run_status` (Phase 3 will), so this is not exploitable today, but the contract that downstream phases will rely on is permissive in a way that contradicts the comment "auto-fills `finished_at` on terminal statuses" — the implication is "and leaves it alone otherwise," but the actual behavior is "leaves it alone even when the new state is non-terminal," which produces the inconsistency above.

**Fix:** Either (a) clear `finished_at` when the new status is non-terminal, or (b) reject non-monotonic transitions outright. Option (a) is the smaller change:
```rust
let finished_at_sql: Option<String> = match status {
    RunStatus::Succeeded | RunStatus::Failed => Some(super::strategies::now_rfc3339()),
    RunStatus::Queued | RunStatus::Running => None,  // explicit clear
    _ => unreachable!("phase2_emittable gate above"),
};
let affected = conn.execute(
    "UPDATE runs SET status = ?1, finished_at = ?2 WHERE id = ?3",
    params![status_to_wire(status), finished_at_sql, run_id],
)?;
```
Option (b) is preferable for journal integrity ("모든 실행은 기록으로 남는다") — add a `validate_transition(from, to)` check, return `StateError::InvalidInput` on illegal moves. Recommend deferring the choice to Phase 3 when the lifecycle FSM is wired, but pick one before agents observe the current behavior.

### MR-02: `map_state_error(StateError::Storage)` leaks raw SQLite error text into `data.detail`

**File:** `crates/executor-mcp/src/errors.rs:80-85`
**Issue:** `StateError::Storage(String)` carries the raw `rusqlite::Error::to_string()` (see `From<rusqlite::Error>` in `executor-state/src/error.rs:24-28`), which can include column names, constraint names, and SQLite-internal phrasing (e.g., `"FOREIGN KEY constraint failed"`, `"UNIQUE constraint failed: strategies.name"`). `map_state_error` then forwards that string verbatim into `data.detail` and the human message.

For a local-trust MCP (single agent, single user), this is low-impact. But:
- It couples the wire contract to the SQLite version's exact error phrasing — a `rusqlite` upgrade can silently change agent-visible strings.
- It exposes schema internals (column/constraint names) to whatever consumes the error, which has no legitimate need for them.
- Phase 6+ may surface these errors to log shippers / observability backends that the project does not yet vet.

**Fix:** Convert known SQLite error categories into typed `StateError` variants at the `From<rusqlite::Error>` boundary so `Storage(String)` becomes a true catch-all and rarely fires. Minimum viable fix: in `map_state_error`, replace `data.detail: msg` with a stable category string (`"sqlite_error"`) and only log the raw text via `tracing::warn!`:
```rust
StateError::Storage(msg) => {
    tracing::warn!(detail = %msg, "storage error");
    McpError::new(
        STORAGE_ERROR,
        "storage error".to_string(),
        Some(json!({ "code": "storage_error" })),
    )
}
```
Defer the fuller fix (typed FK / unique-constraint variants) to a later phase if scope-tight; minimum is to stop forwarding raw text to the wire.

## Low

### LR-01: `NameConflict.existing_source_hash` is a misleading alias for `existing_strategy_id`

**File:** `crates/executor-state/src/strategies.rs:101-106`
**Issue:** In the conflict branch, the code constructs:
```rust
StateError::NameConflict {
    attempted_name: name.to_string(),
    existing_strategy_id: active.id.clone(),
    existing_source_hash: active.id,   // <-- same value
    existing_created_at: active.created_at,
}
```
Because `strategy.id == hex(sha256(source))` (D-01), `existing_strategy_id` and `existing_source_hash` are necessarily identical. Carrying both fields suggests they could differ. The `_source_hash` field is then dropped by `map_state_error` (`existing_source_hash: _,` at `errors.rs:63`), confirming it adds no information.

**Fix:** Remove `existing_source_hash` from the `StateError::NameConflict` variant. If a future schema decouples id from source hash, reintroduce it deliberately. Reduces confusion at every call site.

### LR-02: `strategy_get` not-found response loses the requested identifier

**File:** `crates/executor-mcp/src/tools.rs:144-147`
**Issue:** When a `strategy_get` call misses, the tool emits `StateError::NotFound("strategy".into())`, which `map_state_error` renders as `data.resource = "strategy"` — the requested id/name is dropped. Agents repeating the call against many ids cannot correlate which one missed without reparsing their own request. Compare to `execution_get` at `tools.rs:198-201`, which correctly includes the requested run id (`format!("run {}", input.execution_id)`).

**Fix:**
```rust
None => Err(map_state_error(StateError::NotFound(match &input {
    StrategyGetInput::ById { strategy_id } => format!("strategy {strategy_id}"),
    StrategyGetInput::ByName { name } => format!("strategy name '{name}'"),
}))),
```
Note: requires capturing the matched input before it is moved into the closure, or reading the original request payload.

### LR-03: Unnecessary `String::clone` in `read_strategy`

**File:** `crates/executor-mcp/src/resources.rs:121-136`
**Issue:** `read_strategy(uri, id, state)` already takes an owned `String` for `id`. Inside, it does `let id_owned = id.clone();` to move into the `spawn_blocking` closure, but the original `id` is never read again — the clone is gratuitous.
**Fix:**
```rust
async fn read_strategy(uri: String, id: String, state: Arc<Mutex<StateStore>>) -> ... {
    // hex check using &id
    if id.len() != 64 || !id.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)) { ... }
    let row = tokio::task::spawn_blocking(move || {
        let store = state.blocking_lock();
        store.get_strategy_by_id(&id)
    }).await...
}
```
Move `id` directly into the closure. Minor — costs one heap copy per `strategy://{id}` read.

### LR-04: `encode_tags` silently substitutes `"[]"` on serialization failure

**File:** `crates/executor-state/src/strategies.rs:55-57`
**Issue:** `tags.map(|t| serde_json::to_string(t).unwrap_or_else(|_| "[]".into()))` silently maps any error to an empty array. For `Vec<String>`, `serde_json::to_string` is infallible in practice (no `RawValue`, no key types), so the fallback path is unreachable — but if the type ever changes (e.g., tags become structs), corruption would silently produce empty `[]` writes. The `decode_tags` partner (line 59-61) is explicitly documented as a silent-drop choice; the `encode_tags` silence is undocumented and asymmetric.
**Fix:** Either `unwrap()` (truly infallible for `Vec<String>`, document the invariant) or propagate via `Result`:
```rust
fn encode_tags(tags: Option<&[String]>) -> Option<String> {
    tags.map(|t| serde_json::to_string(t).expect("Vec<String> serialization is infallible"))
}
```

## Info

### IN-01: ULID timestamp granularity exceeds `started_at` granularity

**File:** `crates/executor-state/src/runs.rs:71-72`
**Issue:** `Ulid::new()` encodes ms-precision; `now_rfc3339()` truncates to seconds (`SecondsFormat::Secs`). For runs inserted in the same second, sort by `started_at ASC, id ASC` falls back to ULID-Crockford lexicographic ordering, which IS time-ordered within a millisecond bucket — but only because ULID's first 48 bits are ms timestamp. This works as long as the assumption holds; documenting the dependency would help the next reader. The Plan 02-03 deviation note already documented the secondary `id ASC` tie-breaker; consider adding a one-line comment in `list_runs_for_strategy` explaining why the ULID fallback preserves time order.

### IN-02: Implicit FK action on `runs.strategy_id`

**File:** `crates/executor-state/src/schema.rs:25-27`
**Issue:** `strategy_id TEXT NOT NULL REFERENCES strategies(id)` defaults to `ON DELETE NO ACTION ON UPDATE NO ACTION`. CONTEXT §"Soft delete and FK consistency" specifies "RESTRICT or NO ACTION; cascade explicitly forbidden". The default matches the requirement, but making it explicit (`REFERENCES strategies(id) ON DELETE RESTRICT`) eliminates the dependency on SQLite's defaults and self-documents the decision.

### IN-03: `path` from config is not canonicalized or validated before opening

**File:** `crates/executor-mcp/src/config.rs:46-49`, `crates/executor-mcp/src/server.rs:44-46`
**Issue:** `StateConfig.path` is passed as-is to `StateStore::open`. There is no rejection of obviously hazardous values (`/dev/null`, sockets, FIFOs) and no canonicalization. For Phase 2's local-trust model this is acceptable — the operator controls their own `config.toml` — but if Phase 7+ ever loads config from a less-trusted source (env var injection from a parent process, supply-chain), this becomes a path-traversal / type-confusion vector. Worth flagging now so future readers don't assume the path was vetted.

### IN-04: `decode_tags` silent failure documented but untested

**File:** `crates/executor-state/src/strategies.rs:59-61`
**Issue:** Per 02-01-SUMMARY decisions: "tags column: JSON-array TEXT; on read, decode failure silently maps to None — DB is single-writer so corruption is not expected." The behavior is documented but no test pins it. If a future migration writes a malformed tags blob and the silent-drop path triggers, the agent sees `tags: null` without any signal. Add a unit test (`crates/executor-state/tests/strategy_roundtrip.rs`) that plants invalid JSON via raw SQL through `__test_conn` and asserts `get_by_id` returns `tags: None` (locks the contract). Minor.

---

## Cross-Cutting Observations (positive)

- **SQL parameterization:** Every `conn.execute` / `conn.query_row` / `conn.prepare` in `strategies.rs` and `runs.rs` uses `params![...]`. No `format!`-into-SQL anywhere. T-02-01-01 mitigation verified.
- **Pragma ordering:** `open_conn` applies pragmas in the same `execute_batch` call before DDL. FK enforcement is active during all schema operations. Comment explicitly notes the `:memory:` WAL silent-rejection caveat.
- **Async safety:** Every DB call in `tools.rs` and `resources.rs` enters via `tokio::task::spawn_blocking` and acquires `state.blocking_lock()`; the tokio mutex guard never crosses an `await`. RESEARCH Pattern 2 followed exactly.
- **Validation layer:** `validate_register` checks bytes (not chars) for `source` per Pitfall 8, char-counts for `name`/`description`/`tags` per Unicode contract, whitespace-only rejection on name + tags, and includes the violated bound in every error message per D-09b. `validate_strategy_id_format` enforces lowercase hex + length, called before any DB access.
- **Public API hygiene:** `StateStore::__test_conn` and `__test_insert_run_with_time` are `#[doc(hidden)] pub` with `__test_` prefix and explicit comments naming them test-only seams. Free repository functions (`register`, `list`, etc.) are `pub(crate)`. `Run`, `Strategy`, `StrategySummary`, `RegisterOutcome` are appropriately public for façade use. No accidental `pub` exposure observed.
- **Error propagation:** `From<rusqlite::Error> for StateError` is the single funnel; no `unwrap()` on rusqlite results in production paths. `map_state_error` covers all four `StateError` variants with no fallthrough.
- **Config robustness:** `parse_cli_config_path` correctly handles both `--config PATH` and `--config=PATH` (closes Phase 1 IN-01). `Config::default()` boots cleanly when the file is absent (D-03e). `deny_unknown_fields` on every struct.

## What I Did Not Find

- No SQL injection (all parameterized).
- No `unsafe` blocks anywhere in scope.
- No `unwrap()`/`expect()` on values derived from user input.
- No hardcoded secrets or API keys.
- No `dbg!`/`println!`/`eprintln!` (clippy denylist + per-crate `#![deny]` attribute both clean per 02-01-SUMMARY).
- No path traversal in `strategy://{id}` reads — the 64-hex regex eliminates `/` and `..` before any DB access.
- No race between `register`'s name-conflict check and the INSERT — guaranteed by the single-`Mutex<Connection>` invariant (T-02-01-06 accepted in CONTEXT).
- No leak of `source` payload through `strategy_list` — explicit column projection at `strategies.rs:134-140`, schema type `StrategyListItem` has no `source` field.

---

_Reviewed: 2026-04-27_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
