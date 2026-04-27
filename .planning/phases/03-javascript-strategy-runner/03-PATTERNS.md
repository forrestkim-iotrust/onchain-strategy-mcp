---
phase: 03-javascript-strategy-runner
artifact: PATTERNS
status: complete
mapped: 2026-04-27
files_classified: 14
analogs_found: 13
no_analog: 1
upstream:
  - .planning/PROJECT.md
  - .planning/REQUIREMENTS.md  (STR-03/04/05, STJ-03/04 target this phase)
  - .planning/research/STACK.md (rquickjs 0.11)
  - .planning/phases/02-strategy-state-and-journal/02-01-SUMMARY.md
  - .planning/phases/02-strategy-state-and-journal/02-02-SUMMARY.md
  - .planning/phases/02-strategy-state-and-journal/02-03-SUMMARY.md
downstream:
  - 03-PLAN.md (gsd-planner consumes this for per-task "create X mirroring Y" instructions)
---

# Phase 03 — JavaScript Strategy Runner: Pattern Map

Phase 3 introduces:
1. A new `executor-runtime` (or `strategy-js`) crate hosting the rquickjs-backed JS sandbox + `ctx` host bindings.
2. Schema/repo extension in `executor-state` for journal entries (source-reads, action-returns, validation-errors per STJ-03/04).
3. Wiring of a real `strategy_run` (or `strategy_run_once`) handler in `executor-mcp` that replaces the current `unimplemented_err(-32010)` placeholder.
4. Sandbox / runtime-specific MCP error variants (timeout, oom, return-shape invalid, host-violation, source-deleted).
5. Unit + integration tests proving sandbox boundaries, lifecycle persistence, and end-to-end stdio.

Every analog below points at a concrete file and line range Phase 3 should mirror, not invent.

---

## 1. Existing-Code Analogs Table

| New / modified file | Closest existing analog | Match | Convention to mirror |
|---|---|---|---|
| `crates/executor-runtime/Cargo.toml` (new crate) | `crates/executor-state/Cargo.toml:1-28` | exact (greenfield crate scaffold) | `version/edition/license.workspace = true`; `[lints] workspace = true`; per-crate dep pins (NOT promoted to workspace); `executor-core = { path = "../executor-core" }` for shared schemas. Devs: `tempfile = "3"` only. |
| `crates/executor-runtime/src/lib.rs` (new) | `crates/executor-state/src/lib.rs:1-22` | exact | First line `#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]`; module doc-comment lists all submodules with their D-XX decision references; `pub use` re-exports the public façade type + the typed error. |
| `crates/executor-runtime/src/error.rs` (new) | `crates/executor-state/src/error.rs:1-29` | exact | `#[derive(Debug, thiserror::Error)]` enum named `RuntimeError`; one variant per failure category with `#[error("…: {0}")]`; provide `impl From<rusqlite::Error>` analogue for upstream library errors (e.g. `From<rquickjs::Error>` if needed); module-level doc-comment naming the consuming crate (`executor-mcp::errors::map_runtime_error`). |
| `crates/executor-runtime/src/sandbox.rs` (new — rquickjs + ctx host) | NO ANALOG (rquickjs-specific) | — | See "No Analog" section. Use STACK.md `rquickjs 0.11` and Phase 3 RESEARCH.md once written. **Public surface should mirror `executor-state::store.rs:18-118`**: a single owning struct (`SandboxRuntime` or `JsRunner`) with synchronous façade methods that callers wrap in `tokio::task::spawn_blocking`. Holding rquickjs `Context` across `await` is forbidden (same shape as `runs.rs:1-11` Pitfall 4 note). |
| `crates/executor-state/src/schema.rs` (modified — add `journal_entries` table) | `crates/executor-state/src/schema.rs:11-46` | exact | Append `CREATE TABLE IF NOT EXISTS journal_entries (...)` to `SCHEMA_SQL` const; **idempotent** (`IF NOT EXISTS`); FK `run_id REFERENCES runs(id)`; one supporting `CREATE INDEX IF NOT EXISTS idx_journal_run_id ON journal_entries(run_id)` (mirrors line 33 `idx_runs_strategy_id`). PRAGMA block at lines 39-43 stays untouched. Pitfall 1 (PRAGMA before DDL) remains satisfied because new DDL is appended after the existing `execute_batch` call sequence. |
| `crates/executor-state/src/journal.rs` (new module) | `crates/executor-state/src/runs.rs:1-193` | exact | Module-level doc-comment listing D-references; bare `pub(crate)` free functions (`insert_entry`, `list_for_run`, `count_for_run`); ULID id (`ulid::Ulid::new().to_string()` — runs.rs:71); RFC3339 timestamps via `super::strategies::now_rfc3339` (runs.rs:74); all SQL parameterised with `params!` (no string formatting); `OptionalExtension` for nullable lookups (runs.rs:127-145); `phase3_emittable`-style boundary gate if any entry kinds are reserved for later phases (mirror `phase2_emittable` at runs.rs:62-68 + executor-core/.../execution.rs:39-51). |
| `crates/executor-state/src/store.rs` (modified — façade methods) | `crates/executor-state/src/store.rs:75-117` | exact | Add a `// ---- Journal façade ----` section after the existing `// ---- Run façade ----`; one façade method per repo function; `&self` for reads, `&mut self` for writes; delegate to free functions; if any test-only seam is needed, follow the `__test_insert_run_with_time` pattern (store.rs:90-98) — `#[doc(hidden)] pub fn __test_*` with a doc-comment explaining the determinism need. |
| `crates/executor-state/src/error.rs` (modified — new variants) | `crates/executor-state/src/error.rs:4-22` | exact | Append (do not rename) variants to existing `StateError`. Phase 2 uses Storage / NotFound / NameConflict / InvalidInput; if a journal-specific failure mode is genuinely distinct (e.g. `JournalCorrupt`), add a fifth variant. Otherwise reuse `Storage` / `NotFound`. **Do not introduce a separate `JournalError` enum** — Phase 2 explicitly chose one error type per crate (decisions in 02-01 SUMMARY frontmatter). |
| `crates/executor-state/src/lib.rs` (modified) | `crates/executor-state/src/lib.rs:12-21` | exact | Add `pub mod journal;` next to `pub mod runs;` and `pub use journal::{JournalEntry, JournalRepo};` next to the `runs` re-export. |
| `crates/executor-core/src/schema/execution.rs` (modified — `StrategyRunInput` + `StrategyRunResponse`) | `crates/executor-core/src/schema/strategy.rs:11-25` (input) and `:50-66` (response); `crates/executor-core/src/schema/execution.rs:53-64` (response with optional fields) | exact | `#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]` + `#[serde(deny_unknown_fields)]` + `#[schemars(description = "…")]`; per-field `#[schemars(description = …)]`; `#[serde(default, skip_serializing_if = "Option::is_none")]` for optionals; **`StrategyRunOnceInput` already exists at strategy.rs:34-39** — extend (don't duplicate) it if the agent-facing input shape is the same; otherwise add a new sibling struct in the same module. |
| `crates/executor-core/tests/schema_snapshots.rs` (modified — golden tests) | `crates/executor-core/tests/schema_snapshots.rs:50-124` | exact | One `#[test] fn <type>_schema_stable()` per new struct calling `assert_schema_matches_golden(<Name>, schema_for!(<Type>))`; commit the golden under `crates/executor-core/tests/schemas/<Name>.json`; refresh via `UPDATE_SCHEMAS=1 cargo test -p executor-core --test schema_snapshots` (snapshots.rs:8). |
| `crates/executor-mcp/src/errors.rs` (modified — `map_runtime_error` + new codes) | `crates/executor-mcp/src/errors.rs:18-86` | exact | Add named `pub const` ErrorCode constants in the unallocated `-32017..-32099` range (NEXT free is `-32017` — see "Error Code & Mapping" below). Add `pub fn map_runtime_error(e: RuntimeError) -> McpError` mirroring the structure of `map_state_error` at lines 53-86: one match arm per `RuntimeError` variant, each producing `(code, message, Some(json!({"code": "<snake>", ...})))`. Add unit tests in the existing `mod tests` block at lines 110-181 (one test per branch — see lines 124-180 pattern). |
| `crates/executor-mcp/src/validation.rs` (modified) | `crates/executor-mcp/src/validation.rs:8-12` (constants), `:14-67` (`validate_register`), `:86-185` (tests) | exact | Add `MAX_*` constants for any new bound (e.g. max simulated source-reads per run, max journal entries per run); add `validate_strategy_run(&StrategyRunInput) -> Result<(), String>` if any params need bounds beyond the schema; **byte-vs-char rule** at line 19 / Pitfall 8 (source uses `.len()`, names use `.chars().count()`) is non-negotiable. |
| `crates/executor-mcp/src/tools.rs` (modified — replace placeholder `strategy_run_once`) | `crates/executor-mcp/src/tools.rs:46-98` (`strategy_register` body) **for the validate→spawn_blocking→map_error→shape pattern**; lines 215-224 (current placeholder) for the `#[tool(name=…)]` macro it replaces | exact | The 4-step pattern from 02-02-SUMMARY:80 — `validate(input).map_err(invalid_params)?` → `tokio::task::spawn_blocking(move \|\| { let mut store = state.blocking_lock(); … })` → `.await.map_err(\|e\| storage_error(format!("spawn_blocking join: {e}")))?` → `.map_err(map_state_error)?` (or `map_runtime_error`) → shape into typed response → `json_result(&resp)`. **Pitfall 4** (mutex never held across await) verified by grep: `grep -c spawn_blocking tools.rs` (= 6 in Phase 2; will increase by N for Phase 3 paths). The runtime call (which can be slow) MUST be inside `spawn_blocking` for the same reason DB calls are. |
| `crates/executor-mcp/src/server.rs` (modified — add runtime field) | `crates/executor-mcp/src/server.rs:34-55` | exact | If `executor-runtime` carries owned state, add a second `Arc<tokio::sync::Mutex<…>>` field next to `state` (line 38). Construct it in `new()` (lines 44-54) — fallible construction stays inside `Result<Self>`. **Do NOT** reintroduce `Default for ExecutorServer` or a no-arg `new()` (decisions in 02-02 SUMMARY:44). Plumb it through `read_resource`/tool handlers via `self.runtime.clone()` mirroring `self.state.clone()` at line 112. |
| `crates/executor-mcp/src/resources.rs` (modified — `journal://{run_id}` activation) | `crates/executor-mcp/src/resources.rs:91-119` (URI dispatch) and `:121-167` (the live `read_strategy` body) | role-match | Replace the phase-gated branch at lines 109-114 with a real reader. Mirror `read_strategy` structure: boundary id-shape check first (ULID 26 chars vs 64-hex — see resources.rs:129 + validation.rs:70-84 for the strategy variant), then `spawn_blocking` + `state.blocking_lock()` + new façade method (e.g. `list_journal_for_run`), then serialise to text body via `ResourceContents::text(body, uri).with_mime_type("application/json")` (resources.rs:163; rmcp 1.5 builder per 02-02 SUMMARY:209). Update the `with_instructions` blurb in server.rs:76-86 to flip `journal://` from "phase-gated" to "Phase 3+ live". |
| `crates/executor-mcp/tests/stdio_handshake.rs` (modified — `strategy_run` end-to-end) | `crates/executor-mcp/tests/stdio_handshake.rs:888-990` (`run_roundtrip_insert_get_update_status` — multi-spawn cross-process roundtrip) | exact | Use `tempfile::tempdir()` for the DB path (line 892), drive the runtime through MCP `tools/call`, then assert via `execution_get` that status transitioned `queued`→`running`→`succeeded`/`failed`, that `journal://{run_id}` returns the expected entries, and that `finished_at` is populated only on terminal status. Helper imports come from `common::{spawn_server_with_state, call_tool, extract_json_result, initialize, send, recv}` (common/mod.rs:73-167). Always end with `proc.child.kill().await?;`. |
| `crates/executor-runtime/tests/sandbox_boundary.rs` (new) | `crates/executor-state/tests/strategy_roundtrip.rs:1-7` (header + common import) and `crates/executor-state/tests/run_base_model.rs:1-15` (seed helper pattern) | role-match | `mod common;` in the file head; `common::fresh_*()` constructor (mirror `fresh_memory_store` at `tests/common/mod.rs:7-9`); one `#[test]` per invariant with snake_case names matching the assertion (e.g. `sandbox_blocks_filesystem_access`, `sandbox_rejects_non_array_return`, `sandbox_times_out_at_default_budget`); panic messages on assertion failure include the value (`got: {x}`) per validation.rs:127 / 137 style. |
| `crates/executor-state/tests/journal_repo.rs` (new) | `crates/executor-state/tests/run_base_model.rs:1-78` (seed strategy → seed run → exercise repo) | exact | `mod common;` import; `seed_strategy` + `seed_run` helpers building on `fresh_memory_store`; one test per CRUD shape (insert/list/count/FK-violation); FK enforcement test using raw `__test_conn()` (run_base_model.rs:266-279). |

---

## 2. Workspace Dependency Conventions

Per **02-01 SUMMARY frontmatter (`workspace deps NOT promoted`)**, the workspace has explicitly chosen a **per-crate-only pinning policy** for everything outside the Phase-1 base set. The current shared deps in `Cargo.toml:10-20` are:

```
rmcp, schemars, serde, serde_json, tokio, tracing, tracing-subscriber, anyhow, thiserror, toml
```

Phase 2 added `rusqlite / sha2 / hex / ulid / chrono / tempfile` to **only `executor-state/Cargo.toml`** (and `tempfile` again in `executor-mcp/Cargo.toml [dev-dependencies]`) — **none** were promoted to `[workspace.dependencies]`. The justification is recorded in `executor-state/Cargo.toml:11-13` as a code comment.

**Phase 3 directive:** keep `rquickjs` (and any sibling like `tokio-util` if needed for cancellation) declared **only inside `crates/executor-runtime/Cargo.toml`**. Promotion to `[workspace.dependencies]` only becomes justifiable when ≥2 crates consume the same dep — that won't happen until Phase 4 wires `ctx` from `executor-runtime` into `executor-evm`. Preserve the comment-block precedent at `executor-state/Cargo.toml:11-13`:

```toml
# New deps (rquickjs/...) are intentionally NOT promoted to workspace
# dependencies — only `executor-runtime` consumes them today, mirroring
# the Phase 1 [logging]-only and Phase 2 rusqlite-only precedent.
```

The shared deps the new crate **must** declare via workspace inheritance (mirroring `executor-state/Cargo.toml:21-24`):

```toml
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
```

`schemars` only if the crate exports its own JsonSchema-derived types (likely yes if it surfaces a runtime-config struct).

---

## 3. Test Harness Conventions

Two distinct harnesses exist and Phase 3 must use **both**.

### 3a. State / runtime crate tests — in-process

**Reference:** `crates/executor-state/tests/common/mod.rs:1-25`, `tests/strategy_roundtrip.rs`, `tests/run_base_model.rs`, `tests/partial_index_behaviour.rs`.

Conventions:
- Each test file starts with `mod common;` and imports `common::fresh_memory_store` (or its Phase 3 analog: `fresh_runtime`, `fresh_runtime_with_store`).
- `:memory:` SQLite for purity; `tempfile::tempdir()` only when the test must survive a `Drop`-and-reopen cycle.
- One `#[test]` per invariant, snake_case named after the assertion.
- Test-only seams on the production type are `#[doc(hidden)] pub fn __test_*` with a doc-comment explaining the determinism need (store.rs:88-98 is the canonical example).
- FK / SQL invariants exercised through the `__test_conn` raw accessor (`partial_index_behaviour.rs:13-18`); production code goes through the typed façade.
- Future-reserved enum variants are gated by a `phaseN_emittable()` method on the enum, with the gate consulted at the boundary (`runs.rs:62-68` + `core::execution.rs:39-51`).

### 3b. MCP stdio integration tests — out-of-process

**Reference:** `crates/executor-mcp/tests/common/mod.rs:13-167`, `tests/stdio_handshake.rs`.

Conventions:
- `spawn_server_with_state(db_path)` (common/mod.rs:73-113) writes a temp `[state]` config and exports `EXECUTOR_CONFIG`. **Always use this**, never the bare `spawn_server` (the latter is now only safe for surface-shape tests that don't touch storage).
- `tempfile::NamedTempFile` for config; `.into_temp_path().keep()` so the child can read after parent's auto-delete guard would normally drop (common/mod.rs:86; decision recorded at 02-02 SUMMARY:49).
- For DB path, `tempfile::tempdir()` (NOT `NamedTempFile`) so SQLite WAL+shm sidecars stay co-located and clean up on `Drop` (02-02 SUMMARY:49).
- `initialize()` + then `tools/call` (`call_tool` helper at common/mod.rs:116-131); response shape extracted via `extract_json_result(&r)` (common/mod.rs:135-141).
- Every test ends with `proc.child.kill().await?;`.
- Cross-process state setup pattern (open `StateStore` directly → mutate → drop → spawn server → observe) is established at `stdio_handshake.rs:887-990` and is the canonical approach for "seed before observation" tests.
- `recv` (common/mod.rs:55-68) asserts every stdout line is JSON-RPC 2.0 — D-05 / Pitfall 1 regression guard.

### 3c. Schema golden tests

`crates/executor-core/tests/schema_snapshots.rs:27-48` — `assert_schema_matches_golden` helper, `UPDATE_SCHEMAS=1 cargo test …` to refresh. Phase 3 must add a golden for every new public type in `executor-core::schema::*`. **Lock the wire shape at first introduction**, mirroring 02-01's "all 7 RunStatus variants declared even though Phase 2 only emits 4" decision (02-01 SUMMARY:30, 02-01 SUMMARY:113, core::execution.rs:24-37).

---

## 4. Error Code & Mapping Conventions (carry forward from Phase 2)

### Allocated wire codes

| Code | Constant | Purpose | Source |
|---|---|---|---|
| `-32602` | `INVALID_PARAMS` | JSON-RPC standard, D-09 validation failures | errors.rs:33 |
| `-32010` | `UNIMPLEMENTED_CODE` (private) | Phase-gated tools | errors.rs:24 |
| `-32014` | `STORAGE_NOT_FOUND` | Strategy / run miss | errors.rs:27 |
| `-32015` | `STORAGE_NAME_CONFLICT` | `strategy_register` active-name collision | errors.rs:29 |
| `-32016` | `STORAGE_ERROR` | SQLite / I/O / spawn_blocking-join failures | errors.rs:31 |
| `-32002` | (rmcp built-in `resource_not_found`) | Resource lookup miss | resources.rs:104, 110, 116 |

### Phase 3 allocation guidance (NEXT free codes start at `-32017`)

The orchestrator brief listed 4 categories the planner needs to assign codes to. **No specific codes are pre-locked in 02-CONTEXT.md** — they are open for Phase 3 to assign in its own CONTEXT/PLAN. Recommended (sequential, contiguous):

| Suggested code | Suggested constant | Suggested `data.code` | Failure category |
|---|---|---|---|
| `-32017` | `RUNTIME_TIMEOUT` | `runtime_timeout` | sandbox exceeded execution budget |
| `-32018` | `RUNTIME_OOM` | `runtime_oom` | sandbox memory budget exceeded |
| `-32019` | `RUNTIME_RETURN_INVALID` | `runtime_return_invalid` | strategy returned non-`Action[]` / non-`noop` |
| `-32020` | `RUNTIME_HOST_VIOLATION` | `runtime_host_violation` | strategy attempted forbidden host capability |
| `-32021` | `RUNTIME_SOURCE_DELETED` | `runtime_source_deleted` | invoked a soft-deleted strategy (locked at 02-CONTEXT:56 as "rejected like -32010") |

Phase 3 PLAN may renumber if collisions are discovered with rmcp internals — the audit pattern is at 02-02 SUMMARY:213-215 (`grep -r 'ErrorCode(-3201X)' ~/.cargo/registry/src/index.crates.io-*/rmcp-1.5.0/`).

### Mapping function shape (Phase 3 mirrors `map_state_error`)

`crates/executor-mcp/src/errors.rs:53-86` is the template. Every variant returns:

```rust
McpError::new(
    NUMERIC_CODE,
    format!("human-readable: {detail}"),
    Some(json!({ "code": "snake_case_string", /* structured fields agents bind on */ })),
)
```

**Hard rules:**
- Numeric `error.code` for JSON-RPC clients; **stable** `data.code` snake_case string for agent matching (errors.rs:11-15).
- Free-form `detail` field for the raw error message — never put a stack trace or panic payload in `data` (PII / safety).
- Add unit tests in the same `mod tests` block (errors.rs:110-181 pattern) — one per variant — asserting BOTH `e.code == EXPECTED_CODE` and `data["code"] == "<snake>"`.

---

## 5. Commit / Branch / Tracking Conventions

From `git log --oneline -25`:

### Conventional-commits scope format

```
<type>(<phase>-<plan>): <imperative summary>
```

Concrete examples in-tree:
- `feat(02-01): scaffold executor-state crate with schema + StateStore + StateError` (`9201af1`)
- `feat(02-02): wire StateStore into MCP tools + add error mapping & validation (Tasks 1+2)` (`3ee27fa`)
- `test(02-03): end-to-end execution_get roundtrip + RunStatus future-variants` (`c5fa5fb`)
- `docs(02): plan phase 2 (3 plans, research resolved, validation map)` (`9738cb0`)

**Phase 3 scope tags:** `feat(03-NN)` / `test(03-NN)` / `docs(03-NN)` / `docs(03)` for phase-level artefacts. NN is the per-plan two-digit ordinal.

### Per-task collapse rule

When two tasks must land together to keep the tree green (e.g. `server.rs` field add + tool body update), the plan note explicitly authorises a combined commit, recorded in the SUMMARY frontmatter `decisions:` block (02-02 SUMMARY:47-48 is the precedent). Default is one commit per task.

### State-doc updates after each plan

After every plan completes, **update `.planning/REQUIREMENTS.md` traceability table and `.planning/PROJECT.md` Active list** in the final `docs(03-NN)` commit (e.g. `2831418` — "complete plan summary, advance state, mark STR-01/STR-02/STJ-01 closed"). Phase 3 will close STR-03/04/05 and STJ-03/04 in stages — track state doc changes per-plan, not phase-end.

### No-claude-mention rule

Per global CLAUDE.md, commit messages MUST NOT reference Claude. The two-digit phase prefix is the only required scope.

### Logging convention

`tracing::info!`/`debug!`/`warn!`/`error!` only; **never** `println!`/`eprintln!`/`dbg!` (workspace-level clippy denylist at `Cargo.toml:27-31` PLUS per-crate `#![deny]` tripwires at `executor-state/src/lib.rs:1` / `executor-mcp/src/lib.rs:1` / `executor-mcp/src/main.rs:1`). The same `#![deny]` MUST be applied to `executor-runtime/src/lib.rs`. Tracing is initialised once in `executor-mcp::logging::init` (logging.rs:14-22) — writer is `std::io::stderr` (logging.rs:19, **load-bearing for D-05/MCP-01**). Production tracing emit-site precedent: `crates/executor-mcp/src/main.rs:11-15` uses structured fields (`version = …`, `state_path = …`).

### Test naming convention (verified across Phase 2 tests)

Two patterns are in use; both are acceptable. Phase 3 should pick **one per file** for consistency:
- **Verb-first:** `register_then_get_by_id_roundtrip`, `list_excludes_source_column`, `soft_delete_is_idempotent` (strategy_roundtrip.rs).
- **Subject-first action:** `update_run_status_sets_finished_at_on_succeeded`, `list_runs_for_strategy_orders_by_started_at_asc`, `run_roundtrip_insert_get_update_status` (run_base_model.rs, stdio_handshake.rs).

Both are snake_case, both name the assertion, neither uses `it_*` / `should_*` / `test_*` prefixes — those are explicitly **NOT** in use here.

---

## 6. Anti-Patterns to Avoid (lessons from Phase 2 deviations + summaries)

Drawn from `02-01-SUMMARY.md "Deviations"`, `02-02-SUMMARY.md "Deviations from Plan"`, `02-03-SUMMARY.md "Pitfalls Encountered"`:

1. **Do not hold the tokio mutex across an `await`.** `state.blocking_lock()` is called *inside* `spawn_blocking` (tools.rs:60-69 is the canonical shape). Reviewer can grep: `grep -B1 'await' src/tools.rs | grep blocking_lock` should produce zero matches.
2. **Do not write logs to stdout.** Per-crate `#![deny(clippy::print_stdout, ...)]` exists for a reason. The `with_writer(std::io::stderr)` line at `logging.rs:19` is annotated "load-bearing — do not remove". The new crate gets the same `#![deny]`.
3. **Do not introduce a separate error enum per module.** `executor-state` deliberately uses one `StateError` for the whole crate (02-01 SUMMARY:96). `executor-runtime` follows: one `RuntimeError`.
4. **Do not assume `journal_mode = WAL` against `:memory:`** (Pitfall 3 in schema.rs:5). Tests against `:memory:` MUST NOT assert WAL is enabled — SQLite silently rejects. The pragma block at schema.rs:39-43 already handles this gracefully.
5. **Do not split a tasks-2-and-3 commit if the tree would be red between them.** The 02-02 plan flagged this explicitly (server.rs field add + tool body update); the SUMMARY frontmatter `decisions:` block records the authorisation. Phase 3 plans should pre-flag any such collapse.
6. **Do not assume schema goldens use a single `enum[]` array.** schemars 1.x emits `oneOf: [{enum:[...]}, {const:...}, ...]` for some enum shapes. Phase 02-03 (`run_status_schema_includes_future_variants`) had to walk BOTH `enum` arrays and `const` strings (stdio_handshake.rs:1009-1033 is the canonical walker). Reuse that walker if Phase 3 needs to inspect any new RunStatus-shaped enum.
7. **Do not assume `now_rfc3339()` has sub-second granularity.** strategies.rs:51-53 returns seconds-only RFC3339. Tests that depend on ordering need the `__test_*_with_time` seam pattern (store.rs:88-98 + tests/run_base_model.rs:174-188). Sleep-based ordering is rejected (02-03 SUMMARY:39).
8. **Do not pre-allocate Phase 5/6 enum variants without an emittability gate.** `RunStatus` declared all 7 variants at Phase 2 *and* added `phase2_emittable()` (core/execution.rs:39-51) consumed at the boundary (runs.rs:62-68, runs.rs:108-114). Any new enum (e.g. `JournalEntryKind`) MUST follow the same lock-now-gate-now pattern if some variants are reserved for later phases.
9. **Do not use `ReadResourceResult { contents }` struct literal** — rmcp 1.5 marks the type `#[non_exhaustive]` (02-02 SUMMARY:46, 209-211). Use `ReadResourceResult::new(vec![...])`. Same for any rmcp 1.5 result types Phase 3 touches.
10. **Do not re-introduce `Default for ExecutorServer`.** Phase 2 deliberately removed it (server.rs:57-61) because constructors that open external resources (SQLite, JS runtime, EVM RPC) are fallible. Phase 3's runtime field opens an rquickjs runtime — likewise fallible. Keep the constructor `pub fn new(&Config) -> anyhow::Result<Self>`.
11. **Do not re-`SELECT *` projections in repo functions.** `strategies::list` at strategies.rs:129-155 is explicit-column to keep `source` out of list responses (D-07a, T-02-01-03). Journal listings have a similar attack surface — large `payload` blobs should be excluded from list-shape queries unless explicitly requested.

---

## 7. No-Analog Files (planner falls back to RESEARCH.md once written)

| File | Reason | Substitute |
|---|---|---|
| `crates/executor-runtime/src/sandbox.rs` | rquickjs has no in-tree precedent. Phase 1+2 are pure-Rust crates (rmcp + rusqlite); rquickjs introduces a foreign embedded VM with its own `Context` / `Runtime` / `Persistent` borrow semantics that no existing analog covers. | The crate's **public surface** (`SandboxRuntime` struct, façade methods) still mirrors `executor-state::store.rs:18-118` — single-owner struct with synchronous methods called from `spawn_blocking`. The **internal rquickjs handling** must come from Phase 3 RESEARCH.md (to be produced by the phase-researcher) covering: `Runtime::new` + `Context::full` lifetime, `Context::with` borrow semantics, `Persistent<Function>` vs handle invalidation between calls, memory limit (`Runtime::set_memory_limit`), and interrupt handler (`Runtime::set_interrupt_handler`) for the timeout budget. The Pitfall-list discipline (one numbered Pitfall per gotcha, with file-line citations in code comments) at `runs.rs:1-11` and `schema.rs:1-6` should be replicated in `sandbox.rs:1-N` once research lands. |

---

## 8. Cross-Cutting Pattern Index (quick reference for the planner)

| Concern | Source file:line | Apply to |
|---|---|---|
| Async DB call → `spawn_blocking` + `blocking_lock` | tools.rs:60-72 (template) | every new tool handler in tools.rs and resource handler in resources.rs |
| Typed input validation | validation.rs:14-67 | every new tool input that has bounds beyond schema |
| StrategyId boundary check | validation.rs:70-84 + resources.rs:128-134 | tool handlers (→ `invalid_params`) + resource handlers (→ `resource_not_found`) — different surfaces, same regex |
| MCP error mapping | errors.rs:53-86 | new `map_runtime_error` |
| `#[doc(hidden)] pub fn __test_*` test seam | store.rs:88-98 | any Phase 3 production type that needs deterministic test fixtures |
| Future-reserved enum variants gate | core/execution.rs:24-51 + state/runs.rs:62-68 | new enums (e.g. JournalEntryKind) where some variants are reserved |
| Multi-spawn cross-process roundtrip | stdio_handshake.rs:887-990 | end-to-end strategy_run integration test |
| Schema golden file + walker | schema_snapshots.rs:27-48 + stdio_handshake.rs:992-1054 | every new public schema type |
| Workspace clippy denylist + per-crate `#![deny]` | Cargo.toml:27-31 + state/lib.rs:1 + mcp/lib.rs:1 | new `executor-runtime/src/lib.rs:1` |
| Cargo.toml comment-block justifying per-crate-only deps | state/Cargo.toml:11-13 | new `executor-runtime/Cargo.toml` for rquickjs |

---

## Mapping Coverage

- Files classified: 14 new-or-modified (excluding the schema goldens, which are a generated set per new type)
- Analogs found: 13 (4 exact-match crate scaffolds, 5 exact-match in-tree extensions, 4 role-match patterns)
- No analog: 1 (`sandbox.rs` — rquickjs-specific)

The planner can write Phase 3 plans entirely against in-tree analogs except for the rquickjs glue, which depends on Phase 3 RESEARCH.md being completed first.
