---
phase: 03-javascript-strategy-runner
applied_at: 2026-04-27T00:00:00Z
review_path: .planning/phases/03-javascript-strategy-runner/03-REVIEW.md
findings_in_scope: 5
findings_addressed:
  - HR-01
  - MR-01
  - MR-02
  - MR-03
  - MR-04
fixed: 5
skipped: 0
deferred:
  - LR-01
  - LR-02
  - LR-03
  - LR-04
  - IN-01
  - IN-02
  - IN-03
status: resolved
---

# Phase 3: Code Review Fix Report

**Applied at:** 2026-04-27
**Source review:** `.planning/phases/03-javascript-strategy-runner/03-REVIEW.md`
**Scope:** HIGH + MEDIUM (default; `--all` not requested)

**Summary:**
- Findings in scope: 5 (1 HIGH + 4 MEDIUM)
- Fixed: 5
- Skipped: 0
- LOW (4) and INFO (3) deferred — out of scope for default fix run

## Fixed Issues

### HR-01: `FORBIDDEN_GLOBALS_SCRUB` ordering — scrub now runs BEFORE `__ctx` injection

**Files modified:** `crates/strategy-js/src/sandbox.rs`
**Commit:** `c563add`
**Applied fix:** Reordered `Sandbox::execute` so the D-11 forbidden-globals
scrub (`FORBIDDEN_GLOBALS_SCRUB` eval) runs BEFORE `c.globals().set("__ctx", …)`
rather than after. A future intrinsic that surfaces a name overlapping a host
binding (e.g. a hypothetical `__ctx` intrinsic) can no longer silently delete
the host binding via the scrub. D-11 coverage list (`console`, `fetch`,
`setTimeout`, `setInterval`, `setImmediate`, `queueMicrotask`, `XMLHttpRequest`,
`WebSocket`, `process`, `Worker`, `child_process`, `fs`, `Deno`) confirmed
intact at sandbox.rs:302-309. Added comment documenting the ordering rationale.

**Verification:** `cargo test -p strategy-js` → 42 tests pass.

### MR-01: Stop echoing raw rusqlite text in `data.detail` (Phase 2 MR-02 carry-forward)

**Files modified:** `crates/executor-mcp/src/errors.rs`
**Commit:** `3ae28d5`
**Applied fix:** Replaced
```rust
StateError::Storage(msg) => McpError::new(
    STORAGE_ERROR,
    format!("storage error: {msg}"),
    Some(json!({ "code": "storage_error", "detail": msg })),
)
```
with a tracing-routed variant: raw `rusqlite::Error::to_string()` (constraint
names, table names, SQLite-internal phrasing) goes to `tracing::warn!`, and the
wire surfaces a stable taxonomy string `"storage backend error"` in both
`error.message` and `data.detail`. `data.code == "storage_error"` is unchanged
so agent dispatch is preserved. Updated `map_state_error_storage_uses_32016`
test to assert raw text does NOT leak.

**Verification:** `cargo test -p executor-mcp` → 75 tests pass (including the
updated test that now asserts non-leak instead of leak).

### MR-02: Deprecate legacy `StateStore::update_run_status` (D-12 bypass surface)

**Files modified:** `crates/executor-state/src/store.rs`,
`crates/executor-state/src/runs.rs`,
`crates/executor-state/tests/run_base_model.rs`,
`crates/executor-mcp/tests/stdio_handshake.rs`
**Commit:** `a715c53`
**Applied fix:** Marked both `StateStore::update_run_status` (façade) and
`runs::update_run_status` (free function) with `#[deprecated(note = "use
update_run_status_with_transition …")]`. **Deprecation chosen over removal**
because run_base_model.rs (8 call sites) and stdio_handshake.rs (2 call sites)
intentionally exercise the unguarded path to lock pre-D-12 contract semantics
(reserved-variant gate, terminal `finished_at` autofill, NotFound). Test files
opt in via `#![allow(deprecated)]` (run_base_model.rs file-level, with comment
explaining intent) or `#[allow(deprecated)]` (stdio_handshake.rs per-block, with
inline comment). No production caller invokes the deprecated method — only the
transition-guarded variant runs in `tools.rs::strategy_run`.

**Verification:** `cargo build --workspace --all-targets` clean, no warnings;
`cargo test -p executor-state -p executor-mcp` → 115 tests pass.

### MR-03: Propagate serde failures from `record_action` instead of writing fallback `"[]"`

**Files modified:** `crates/executor-state/src/error.rs`,
`crates/executor-mcp/src/errors.rs`,
`crates/executor-mcp/src/tools.rs`
**Commit:** `ae6e02f`
**Applied fix:**
1. Added `StateError::SerializationError(String)` variant (mirrors Phase 2
   `Storage` / `InvalidInput` taxonomy).
2. Wired `map_state_error` to translate it to `STORAGE_ERROR` (-32016) with
   stable wire `data.detail = "journal payload serialization failed"`; raw
   serde error goes to `tracing::warn!` (same discipline as MR-01).
3. Replaced `serde_json::to_string(actions).unwrap_or_else(|_| "[]".into())`
   in `tools.rs::record_action` with `?`-propagation:
   ```rust
   serde_json::to_string(actions).map_err(|e| {
       map_state_error(StateError::SerializationError(format!(
           "journal_actions.payload (Vec<Action>): {e}"
       )))
   })?
   ```

The journal is now never silently corrupted by a swallowed serde failure;
a legitimate empty-array success run is no longer indistinguishable from
a swallowed serialize error. Critical for Phase 4 when `Action` gains
fallibly-serializable variants (`ContractCall` / `RawCall` / etc.).

**Verification:** `cargo test -p executor-state -p executor-mcp` → 115 tests pass.

### MR-04: Add per-run monotonic `seq` column to `journal_logs`

**Files modified:** `crates/executor-state/src/schema.rs`,
`crates/executor-state/src/journal.rs`
**Commit:** `a4bebe6`
**Applied fix:**
1. Schema: added `seq INTEGER NOT NULL` + `UNIQUE (run_id, seq)` to the
   `CREATE TABLE IF NOT EXISTS journal_logs` block. Phase 3 fresh table — no
   data migration needed per user instruction.
2. `record_log` and `record_log_with_time` now derive the next seq via
   `SELECT COALESCE(MAX(seq), -1) + 1 FROM journal_logs WHERE run_id = ?1`.
   Single-writer Phase 3 invariant (`Mutex<Connection>` + `spawn_blocking`)
   makes the SELECT-then-INSERT pair race-free; `UNIQUE (run_id, seq)` is a
   schema-level backstop.
3. `list_logs_for_run` now orders by `(recorded_at ASC, seq ASC)` — same-second
   / same-millisecond log inserts are deterministically insertion-ordered.
4. Added `seq: i64` field to public `LogEntry`.

`journal_actions` writes one row per run from a single handler call, so it
does NOT exhibit the same flaw — left unchanged per user instruction's
conditional ("if the same flaw exists there").

**Verification:** `cargo build --workspace --all-targets` clean; full
workspace `cargo test` → 175 tests pass.

## Skipped Issues

None — all in-scope findings fixed.

## Deferred Issues (out of scope)

LOW and INFO findings deferred per default scope (no `--all` flag):

- **LR-01:** `RuntimeContext::flush` is non-atomic across source-read and
  log inserts (no concurrent run-deletion path in Phase 3).
- **LR-02:** `outcome_from_wire` returns `StateError::Storage` for unknown
  enum strings (cosmetic; with MR-01 applied, the raw enum value no longer
  leaks to wire — `data.detail` is now stable).
- **LR-03:** `chrono` dep in `strategy-js` for one clock provider (taste).
- **LR-04:** Stale doc comment on `Sandbox::execute` (cosmetic).
- **IN-01:** `ctx.now()` snapshot doc nit.
- **IN-02:** Journal FK action default (`NO ACTION`) self-documentation.
- **IN-03:** `qjs_value_to_json` recursion depth limit.

## Final Verification

```
cargo test --workspace            → 175 passed (23 suites, 4.37s)
cargo clippy --workspace --all-targets -- -D warnings → No issues found
```

Both gates green. Commits land on `main` in order:

```
c563add fix(03): HR-01 run D-11 forbidden-globals scrub BEFORE host __ctx injection
3ae28d5 fix(03): MR-01 stop echoing raw rusqlite text in storage_error data.detail
a715c53 fix(03): MR-02 deprecate legacy update_run_status (D-12 bypass surface)
ae6e02f fix(03): MR-03 propagate serde failures from record_action instead of writing fallback "[]"
a4bebe6 fix(03): MR-04 add per-run monotonic seq column to journal_logs
```

**Status:** HR-01 fixed | MR-01..04 fixed | LOW + INFO deferred (out of scope)

---

_Applied: 2026-04-27_
_Fixer: gsd-code-fixer_
_Iteration: 1_
