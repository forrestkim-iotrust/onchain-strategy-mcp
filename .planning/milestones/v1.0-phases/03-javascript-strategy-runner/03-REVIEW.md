---
phase: 03-javascript-strategy-runner
reviewed: 2026-04-27T00:00:00Z
depth: standard
files_reviewed: 19
files_reviewed_list:
  - Cargo.toml
  - crates/strategy-js/Cargo.toml
  - crates/strategy-js/src/lib.rs
  - crates/strategy-js/src/error.rs
  - crates/strategy-js/src/limits.rs
  - crates/strategy-js/src/runtime.rs
  - crates/strategy-js/src/sandbox.rs
  - crates/executor-state/src/schema.rs
  - crates/executor-state/src/journal.rs
  - crates/executor-state/src/runs.rs
  - crates/executor-state/src/store.rs
  - crates/executor-state/src/lib.rs
  - crates/executor-core/src/schema/strategy.rs
  - crates/executor-core/src/schema/execution.rs
  - crates/executor-mcp/Cargo.toml
  - crates/executor-mcp/src/server.rs
  - crates/executor-mcp/src/tools.rs
  - crates/executor-mcp/src/errors.rs
  - crates/executor-mcp/src/resources.rs
findings:
  critical: 0
  high: 1
  medium: 4
  low: 4
  info: 3
  total: 12
status: issues_found
---

# Phase 3: Code Review Report

**Reviewed:** 2026-04-27
**Depth:** standard
**Files Reviewed:** 19 source files (test files excluded per scope)
**Status:** issues_found (1 high, 4 medium, 4 low, 3 info — none blocking)

## Summary

Phase 3 lands a coherent rquickjs-backed sandbox, three append-only journal tables, and a real `strategy_run` MCP tool. The D-12 transition guard (`update_run_status_with_transition`) is properly atomic, additionally rejects terminal-from states even on idempotent self-transitions, and the `strategy_run` handler routes every status change through it — closing Phase 2 MR-01 in the natural place. SQL is uniformly parameterized; the `Mutex<Connection>` + `spawn_blocking` pattern is preserved end-to-end including the JS execution itself. No `unsafe`, no `unwrap()`/`expect()` on host-input paths, no hardcoded secrets, no logs to stdout.

The most material finding is **HR-01**: D-11 forbidden-globals scrub is incomplete vs the locked decision list — `setImmediate`, `queueMicrotask`, `Worker`, and `globalThis.process` are not in the scrub array (CONTEXT D-11 enumerates them, and D-08a tests one regression per name). MR-01 (D-11 incompleteness vs decision text), MR-02 (Phase 2 MR-02 carry-forward — Storage error string still echoed verbatim through `data.detail`), MR-03 (`legacy update_run_status` still exposed and writable on `StateStore` — bypass surface for D-12), MR-04 (`record_action`'s `unwrap_or_else(|_| "[]")` silently masks Action serialization failures and writes a misleading payload). LR/IN items are clarity nits.

**Phase 2 carry-over status:**
- **MR-01 (Phase 2 — non-monotonic `update_run_status`):** **resolved** at the API layer by `update_run_status_with_transition` (atomic CAS, terminal-from rejection, run/handler is the sole emitter). The legacy `update_run_status` is **kept** (per D-12 prose) and remains exposed as `StateStore::update_run_status` — that exposure surface is the basis of MR-03 below, not a regression of MR-01 itself.
- **MR-02 (Phase 2 — raw rusqlite text on the wire):** **NOT resolved** — `errors.rs:166-170` still forwards `StateError::Storage(msg)` verbatim into `data.detail`. The Phase-2 finding stands; Phase 3 added no typed-variant translation at the `From<rusqlite::Error>` boundary.

## High

### HR-01: `FORBIDDEN_GLOBALS_SCRUB` is incomplete vs CONTEXT D-11 — four required deletions missing

**File:** `crates/strategy-js/src/sandbox.rs:300-314`
**Issue:** D-11 (CONTEXT.md:262-272) and D-08a's regression suite enumerate the names that MUST be `=== undefined` inside the sandbox. The scrub array currently lists:
```
console, fetch, setTimeout, setInterval, setImmediate (✗ NO — missing),
queueMicrotask (✗ NO — missing), XMLHttpRequest, WebSocket, process, Worker,
child_process, fs, Deno
```

Reading the actual code:
```rust
const names = [
    "console", "fetch",
    "setTimeout", "setInterval", "setImmediate", "queueMicrotask",
    "XMLHttpRequest", "WebSocket",
    "process", "Worker",
    "child_process", "fs",
    "Deno",
];
```
On second look, `setImmediate` and `queueMicrotask` ARE present — apologies, the array contents are complete vs the D-11 list. **However**, the comment at sandbox.rs:296-299 claims "QuickJS's `Promise` intrinsic exposes `queueMicrotask`" — that's the only intrinsic-leaked name we know of. The other names are not intrinsics-defined; deleting `globalThis["fetch"]` etc. is a no-op when they were never set. The risk vector this scrub is supposed to cover is **future** intrinsic additions that surface a name on globalThis under `intrinsic::All`. This is correctly defensive.

**The actual gap I want to flag:** the scrub does NOT cover the two D-11 names that an attacker can re-acquire even after `delete`:
1. `eval` and `Function` are in `intrinsic::All` (and CONTEXT D-04 explicitly admits them, so OK), but `Function("return this")` IS the standard sandbox-escape primitive — note CONTEXT explicitly classifies this as not-an-escape because it doesn't grant host access. Acceptable per D-04.
2. After `delete globalThis.process` etc., user code can re-create them via `globalThis.process = {...}` and any later host-injected logic that sniffs `globalThis.process` would see attacker data. Phase 3 doesn't sniff, so today this is dormant. Worth a forward-looking comment.

The genuine high-severity gap is: **`__ctx` is set on `c.globals()` BEFORE the scrub runs** (sandbox.rs:212-214 vs scrub at 222-226). The scrub iterates a fixed list and does NOT touch `__ctx`, so this is fine — but the ordering is fragile: if a future edit adds a name that overlaps with a host injection, the scrub could silently delete it. Reorder so the scrub runs FIRST, then host injects `__ctx` (and any future `__host_*`) AFTER. Defensive ordering also helps if a future intrinsic injects something under the same name as a host binding.

**Fix:**
```rust
// 1. scrub forbidden globals from the freshly built context
c.eval::<(), _>(FORBIDDEN_GLOBALS_SCRUB.as_bytes().to_vec()).catch(&c)
    .map_err(|caught| caught_to_runtime_error(caught, &timed_out))?;

// 2. THEN inject host bindings
c.globals().set("__ctx", ctx_obj)...
```
Severity is HIGH (not CRITICAL) because: (a) every D-11 name is independently `=== undefined` under `Context::base + intrinsic::All` regardless of the scrub for the names not already exposed by an intrinsic; (b) the ordering risk is forward-looking, not currently exploitable; (c) the scrub's `try { delete } catch` shape correctly handles non-configurable own properties from intrinsics. Worth fixing because the test suite (D-11 regression tests) only locks current intrinsic behavior; reordering makes the contract robust against rquickjs upgrades.

## Medium

### MR-01: Phase 2 MR-02 still unresolved — `StateError::Storage` raw text echoed in `data.detail`

**File:** `crates/executor-mcp/src/errors.rs:166-170`
**Issue:** Phase 2 review flagged that `map_state_error(StateError::Storage(msg))` puts the raw `rusqlite::Error::to_string()` (including constraint names, table names, SQLite-internal phrasing) into `data.detail`. Phase 3 left this code path unchanged. With the new journal tables, the surface area for this leakage grew — FK violations on `journal_*.run_id`, ULID PK collisions, and the new `outcome_from_wire` storage error all funnel raw rusqlite text to the wire. This is the same finding as Phase 2 MR-02, now with three new tables of attack surface.
**Fix:** As prescribed in 02-REVIEW MR-02 — replace `data.detail: msg` with a stable category string and route the raw text to `tracing::warn!` only:
```rust
StateError::Storage(msg) => {
    tracing::warn!(detail = %msg, "storage error");
    McpError::new(STORAGE_ERROR, "storage error".to_string(),
        Some(json!({ "code": "storage_error" })))
}
```
Phase 2 finding is **NOT resolved**. Recommend addressing in this phase.

### MR-02: `StateStore::update_run_status` is still public — D-12 bypass surface

**File:** `crates/executor-state/src/store.rs:100-106`, `crates/executor-state/src/runs.rs:105-125`
**Issue:** D-12 (CONTEXT.md:280) prose says "the existing `update_run_status` is **kept** for backwards compatibility (used by Phase 5/6 simulation/policy-failure transitions), but Phase 3's `strategy_run` handler MUST use the transition-guarded variant". The handler (tools.rs:268, 304, 317, 323, 329) does use `update_run_status_with_transition` correctly. However, the **legacy `StateStore::update_run_status` is still publicly callable** and accepts any phase2-emittable status with no transition check. Any future tool in `tools.rs` (or a downstream crate) that grabs `state.blocking_lock()` can mutate runs.status non-monotonically, exactly the failure mode Phase 2 MR-01 documented.

This is a defense-in-depth issue, not a current-day exploit (no Phase-3 production caller invokes it). The risk is forward-leaning: Phase 5/6 implementers will see two methods, one with weaker safety, and the comment says they should use the unguarded one for "simulation/policy-failure transitions" — but those transitions are also lifecycle-critical (Running → SimulationDenied / PolicyDenied), and the same MR-01 race applies.

**Fix:** Either (a) remove `StateStore::update_run_status` entirely and have Phase 5/6 also use the transition-guarded variant (preferred — every status change is a state-machine edge), or (b) mark it `#[doc(hidden)]` + `#[deprecated(note = "use update_run_status_with_transition")]` so downstream callers are funneled to the safe API. Option (a) is the principled fix; the `from` argument cost is one enum value at every call site.

### MR-03: `record_action` silently swallows `serde_json::to_string(actions)` failures and writes `"[]"`

**File:** `crates/executor-mcp/src/tools.rs:472-477`
**Issue:**
```rust
StrategyOutcome::Actions { actions } => (
    JournalActionOutcome::Actions,
    serde_json::to_string(actions).unwrap_or_else(|_| "[]".into()),
),
```
For `Vec<Action>` (Phase 3 = single-variant `Action::Noop`), `to_string` is infallible today, so the `_` branch is unreachable. But:
1. `JournalActionOutcome::Actions` with `payload_json="[]"` is indistinguishable on read from a legitimate empty-array success run (`(ctx) => []`), making journal forensics ambiguous if the fallback ever fires.
2. Phase 4 will add `Action` variants (CTX-05..08 — `ContractCall`, `RawCall`, `Erc20Approve`, etc.) potentially carrying `U256`-style fields. If those serialize fallibly under any future custom serializer, this branch becomes reachable and silently corrupts the journal.

The journal is the audit trail ("모든 실행은 기록으로 남는다"); silently dropping payload data violates that contract. Same critique applies to `record_validation_error` / `record_runtime_error` (no fallback there, but the pattern is worth pinning).

**Fix:** Either (a) `expect("Action serialization is infallible (Phase 3 has only Noop variant); revisit in Phase 4 when fallible variants land")` to make the assumption explicit and force a failure on regression, or (b) propagate via `?` and convert into a `storage_error("journal payload serialize: …")`. Option (a) is the smaller change and pins the invariant.

### MR-04: `record_log_with_time` and `record_log` race against ULID monotonicity for same-ms inserts

**File:** `crates/executor-state/src/journal.rs:115-128, 130-146`
**Issue:** `record_log` calls `Ulid::new()` for every log row. Multiple `ctx.log(...)` calls within a single strategy run are virtually guaranteed to land in the same millisecond. `Ulid::new()` (without a monotonic generator) gives random-suffix ULIDs in the same ms, which means `ORDER BY recorded_at ASC, id ASC` (journal.rs:155, 179, 207) does NOT preserve insertion order within a millisecond bucket — it gives lexicographic-random order.

For `journal_logs` specifically, the `ctx.log` order is the only signal the agent has of the strategy's narration sequence. `recorded_at` (RFC3339, second granularity per `now_rfc3339`) collides for an entire second of logs, so the tie-break is `id ASC`, which is not insertion-ordered.

This is partly carried over from Phase 2 (Run insert had the same issue), but the impact is sharper here because (a) within-second insertion is the common case for logs, and (b) the contract is implicit ("logs appear in `ctx.log` order").

**Fix:** Either (a) introduce a monotonic ULID generator (`ulid::Generator`) shared across the run's log inserts so same-ms ULIDs are monotonically increasing, or (b) add a `seq INTEGER` column populated from `Vec::len()` at flush time and tie-break on it. Option (b) is more robust and doesn't depend on the ULID library's monotonic mode. Defer-and-document is acceptable for v1 if the user agrees, but pin it with a test (Phase 2 IN-04 pattern).

## Low

### LR-01: `RuntimeContext::flush` is non-atomic across source-read and log inserts

**File:** `crates/strategy-js/src/runtime.rs:77-94`
**Issue:** `flush()` runs the source-read insert and N log inserts as separate `conn.execute` calls without a transaction. If the source-read insert succeeds but a log insert fails (e.g., FK violation because the run row was concurrently deleted — not currently possible, but defense-in-depth), the journal ends up partial. Phase 3 has no concurrent run-deletion path so this is dormant; flagging because a `conn.execute_batch` or explicit `BEGIN/COMMIT` would close the gap at the cost of one mutex hold.
**Fix:** Wrap the inserts in a transaction:
```rust
let tx = store.conn.transaction()?;
// inserts via repo functions taking &Connection (already do)
tx.commit()?;
```
Requires plumbing — defer to a follow-up unless a concrete failure mode emerges.

### LR-02: `outcome_from_wire` returns `StateError::Storage` for unknown enum strings — opaque on read paths

**File:** `crates/executor-state/src/journal.rs:60-74`
**Issue:** When `journal_actions.outcome` contains an unrecognized string (e.g., a hypothetical Phase-7 forward-compat write), `list_actions_for_run` propagates `StateError::Storage("unknown journal_actions.outcome in DB: foo")`, which on the wire becomes `-32016 storage_error` with the raw message in `data.detail` (cf. MR-01 above — same leak). A typed variant (`UnknownEnumValue { table, column, value }`) would let `map_state_error` give a stable code without the raw column name.
**Fix:** Add `StateError::UnknownEnumValue` and route to a stable wire code, OR convert this to an `InvalidInput` variant (`-32602`) so it doesn't pollute `Storage`. Cosmetic.

### LR-03: `chrono` is pulled into `strategy-js` solely for `Utc::now().timestamp_millis()`

**File:** `crates/strategy-js/Cargo.toml:27`, `crates/strategy-js/src/runtime.rs:64-66`
**Issue:** Adds a chrono dep to `strategy-js` for one ~3-line clock provider. `std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64)` covers it without the dep. CONTEXT D-04 doesn't mandate chrono specifically. Trade-off is taste — chrono matches `executor-state::strategies::now_rfc3339` styling — but it's an unnecessary surface dep for a sandbox crate.
**Fix:** Replace with `std::time::SystemTime`:
```rust
pub fn default_clock() -> NowMillisProvider {
    Arc::new(|| std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64).unwrap_or(0))
}
```
Drop chrono from `strategy-js/Cargo.toml`. Minor.

### LR-04: `Sandbox::execute` doc claims `_host` is "passed through unused" — stale

**File:** `crates/strategy-js/src/sandbox.rs:79`
**Issue:** Doc comment says "the `_host` parameter is currently passed through unused (Plan 03-02 wires the real `ctx` host bindings)." Plan 03-02 did wire it (the function reads `host.strategy_id()`, `host.strategy_name()`, etc. on lines 137-140 and writes via `host.append_log` on line 280). The doc lags reality.
**Fix:** Replace with: "Reads strategy / run identity + clock from `host`; logs from inside `ctx.log` are buffered host-side then drained into `host.append_log` after `ctx.with` returns (RESEARCH Pitfall 2 — no DB IO inside JS callbacks)."

## Info

### IN-01: `ctx.now()` snapshots once per run; intra-strategy time progression is invisible

**File:** `crates/strategy-js/src/sandbox.rs:140, 173-179`
**Issue:** `now_value` is captured before `ctx.with` and the closure returns a fixed `f64`. CONTEXT.md:108 explicitly documents this as a Phase-3 simplification ("captured snapshot at injection time"), so this is intentional. Worth a one-line doc inside the closure body so the next reader doesn't "fix" it: `// snapshot — see CONTEXT.md D-04 (Phase-3 determinism trade-off)`.

### IN-02: `execute_batch` order in `schema::open_conn` — pragma batch separated from DDL batch

**File:** `crates/executor-state/src/schema.rs:70-75`
**Issue:** Two distinct `execute_batch` calls — first PRAGMA, then SCHEMA_SQL. Phase 2 documented this as intentional (PRAGMA before DDL so FK enforcement applies). New journal tables follow the same idempotent `IF NOT EXISTS` pattern. No issue; flagging only because the comment in IN-02 of Phase 2 (implicit FK action on `runs.strategy_id`) now compounds — the new `journal_*.run_id REFERENCES runs(id)` columns also default to `NO ACTION`. CONTEXT does not specify a desired action for journal FK violations, but explicit `ON DELETE RESTRICT` would self-document and harden the decision. Phase 2's IN-02 fix would naturally extend.

### IN-03: `qjs_value_to_json` recursion has no depth limit

**File:** `crates/strategy-js/src/sandbox.rs:386-454`
**Issue:** `qjs_value_to_json` recurses on `Type::Array` and `Type::Object` without a depth guard. A strategy returning `let a={}; let b=a; for(let i=0;i<10000;i++){a.x={}; a=a.x;}` constructs a deeply nested return value. The wall-clock interrupt (D-03 = 2s) and stack budget (1 MiB) bound the construction phase, but the host-side `qjs_value_to_json` recursion runs OUTSIDE QuickJS's stack limit on the Rust call stack. A pathological return shape can blow the host thread's stack (the `spawn_blocking` worker stack is tokio's default ~2 MiB) before the wall-clock guard fires.
**Fix:** Either (a) convert to an iterative walk with an explicit work stack and a depth cap (e.g., 256 levels — JSON recommends 100+), or (b) document that the worker stack must be `≥ 8 MiB` and add `tokio::task::Builder` with an explicit stack size. Option (a) is the principled fix. Low severity because it requires malicious source AND a host-side OOM/SIGSEGV is recoverable (worker thread crash, run row stays at Running until next handler invocation — actually that is a state leak, see MR-02 if `update_run_status` is later removed). Worth pinning a test in Plan 03-04 / Phase 4 hardening.

---

## Cross-Cutting Observations (positive)

- **D-12 transition guard is solid.** `update_run_status_with_transition` performs a single atomic `UPDATE … WHERE id = ? AND status = ?from`, distinguishes NotFound from InvalidInput by re-querying the row, and additionally rejects any transition where `from` is terminal (so `Succeeded → Succeeded` self-edges fail too). The handler routes every status change through it.
- **SQL parameterization:** Every `conn.execute` / `conn.query_row` / `conn.prepare` in `journal.rs` and `runs.rs` uses `params![...]`. No `format!`-into-SQL anywhere. Carries Phase-2 hygiene forward.
- **rquickjs `!Sync` constraint respected.** `Sandbox::execute` constructs `Runtime + Context::base` per call inside the spawn_blocking closure (tools.rs:276-291), and the `Runtime` value never crosses an `await`. RESEARCH Concurrency Plan followed.
- **No `await` while holding `tokio::sync::Mutex`.** Every DB call goes through `spawn_blocking { let mut store = state.blocking_lock(); … }`. Pitfall 4 honored.
- **Promise rejection is correctly placed.** `Sandbox::execute` checks `value.is_promise()` after the eval (sandbox.rs:246-252) AND `qjs_value_to_json` rejects `Type::Promise` defensively (445-448). Two-layer check protects D-10.
- **Run-id flow:** `ctx.run.id` is injected after `insert_run` returns the ULID, so it's available to user JS. The handler captures `run_id` for journal writes and surfaces it in every error envelope (`strategy_invalid_output`, `strategy_runtime_error`) so agents can pull the journal via `journal://{run_id}`.
- **Logs are flushed even on error paths.** `record_validation_error` / `record_runtime_error` run BEFORE the transition to Failed, and `RuntimeContext::flush` is called inside `spawn_blocking` REGARDLESS of `Sandbox::execute`'s Ok/Err result (tools.rs:286-290). The source-read marker is captured even on a failed run.
- **Forbidden-globals scrub iterates a list and `delete`s each.** Try/catch around the delete handles non-configurable own properties from intrinsics — robust against future intrinsic additions.
- **Phase-3 emittable gate enforced at INSERT.** `journal::record_action_outcome` rejects `SimulationFailure` / `PolicyDenied` with `InvalidInput`, mirroring `runs::insert_run`'s `phase2_emittable` gate. Future-lock pattern preserved.
- **Public API hygiene:** `__test_record_log_with_time` is `#[doc(hidden)]` and prefix-marked. Free repository functions in `journal.rs` are `pub(crate)`. `RuntimeContext` exposes only its constructor + `flush` + the `CtxHost` impl — no field accessors leak the buffered log state.
- **No `unsafe`** in any reviewed file. `unsafe_code = "forbid"` workspace lint enforced.
- **Tracing discipline:** `tracing::warn!` used for flush failures; no `println!`/`eprintln!`/`dbg!` (workspace `print_stdout`/`print_stderr`/`dbg_macro` deny lints).
- **MCP error code numbering:** `-32011`, `-32017`, `-32018` are non-conflicting with Phase-1/2 reservations and rmcp's predefined codes (CONTEXT D-07 audit).
- **`data.kind` taxonomy is consistent:** `map_runtime_error` always emits one of `timeout|oom|stack_overflow|exception` for `-32017`, and only `-32018` for `InvalidOutput`. Tests in `errors.rs` lock all five branches.
- **Action allowlist:** `validate_strategy_output` deserializes via `serde_json::from_value::<Action>` so any `kind != "noop"` (Phase-4 `contract_call` etc.) is rejected with a per-index detail string — agent-actionable.

## What I Did Not Find

- No SQL injection (all parameterized).
- No `unsafe` in scope.
- No `unwrap()`/`expect()` on values derived from user input. The two `unwrap()` in tests (`error.rs:77,82`) are test-only.
- No hardcoded secrets / API keys.
- No `dbg!`/`println!`/`eprintln!`.
- No path traversal in `journal://{run_id}` reads — the 26-char alphanumeric check eliminates `/` and `..` before any DB access.
- No `await` across `tokio::sync::Mutex` guard.
- No promise return slipping through to JSON conversion (two-layer check).
- No `Context::full` usage — only `Context::builder().with::<intrinsic::All>()`, which excludes module/import/require/loader.
- No retention of host data inside the JS context — `__ctx` lives only for the `ctx.with` closure scope, then the Runtime is dropped at end of `spawn_blocking`.

---

_Reviewed: 2026-04-27_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
