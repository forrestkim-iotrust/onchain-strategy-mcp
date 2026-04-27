---
phase: 03-javascript-strategy-runner
artifact: CONTEXT
status: locked
gathered: 2026-04-27
mode: planner-locked   # /gsd-discuss-phase was skipped — researcher recommendations adopted as defaults
upstream:
  - .planning/PROJECT.md
  - .planning/REQUIREMENTS.md       # STR-03/04/05 + STJ-03/04
  - .planning/ROADMAP.md            # Phase 3 entry, Plans 03-01..03-03
  - .planning/phases/03-javascript-strategy-runner/03-RESEARCH.md
  - .planning/phases/03-javascript-strategy-runner/03-PATTERNS.md
  - .planning/phases/02-strategy-state-and-journal/02-01-SUMMARY.md
  - .planning/phases/02-strategy-state-and-journal/02-02-SUMMARY.md
  - .planning/phases/02-strategy-state-and-journal/02-03-SUMMARY.md
  - AGENTS.md   # rquickjs + strategy-js crate name (line 22, 33)
decisions:
  - D-01: rquickjs 0.11 default-features=false as the sandbox engine
  - D-02: New crate `crates/strategy-js/` (matches AGENTS.md target architecture)
  - D-03: Resource defaults — wall-clock 2s, heap 64 MiB, GC threshold 8 MiB, stack 1 MiB
  - D-04: Phase-3 `ctx` host surface — `ctx.strategy.id`, `ctx.strategy.name`, `ctx.run.id`, `ctx.now()`, `ctx.log(...)`, `ctx.actions.noop()` (Phase-4 surface deferred)
  - D-05: Strategy entry-point — Shape B locked: source MUST evaluate to a function `(ctx) => "noop" | Action[]`
  - D-06: Three separate journal tables — `journal_source_reads`, `journal_actions`, `journal_logs` (researcher Q2)
  - D-07: New MCP error codes — -32011 STRATEGY_DELETED, -32017 STRATEGY_RUNTIME_ERROR (with data.kind), -32018 STRATEGY_INVALID_OUTPUT
  - D-08: `strategy_run` tool input/output contract (replaces Phase-1 `strategy_run_once` placeholder)
  - D-08a: Stdio-integration test cases for D-08 (one per success and error variant)
  - D-09: `strategy_run` parameter validation rules (id format + size bounds)
  - D-10: Promise return values are rejected as STRATEGY_INVALID_OUTPUT
  - D-11: console / fetch / setTimeout / setInterval / require / import / process / globalThis are absent; one regression test per name
  - D-12: Run lifecycle FSM — `Queued → Running → Succeeded | Failed`. Closes 02-REVIEW MR-01 by adding a transition-guarded update API in `executor-state`.
---

# Phase 3: JavaScript Strategy Runner — Context (Locked)

**Status:** locked. `/gsd-discuss-phase` was intentionally skipped per orchestrator brief; the researcher's recommendations in `03-RESEARCH.md` are adopted as **default-locked decisions**, deviating only where REQUIREMENTS.md, AGENTS.md, or PROJECT.md force a different choice.

This document is the agent-facing decision log for Phase 3 plans (03-01 / 03-02 / 03-03). Every plan's `decisions:` frontmatter MUST reference a subset of D-01..D-12 below; every implementation choice in those plans MUST be traceable here.

---

<domain>
## Phase Boundary

Runtime executes a sandboxed JavaScript strategy and journals the run.

**This Phase delivers:**
- New crate `strategy-js/` containing the rquickjs sandbox + `Sandbox` synchronous façade.
- Phase-3 `ctx` host surface (D-04) plus the source-read journal pattern reused by Phase 4+.
- Schema extension in `executor-state` for three new tables (`journal_source_reads`, `journal_actions`, `journal_logs`) with append-only repository methods.
- A real `strategy_run` MCP tool that replaces the Phase-1 `unimplemented_err(-32010)` placeholder, plus three new MCP error codes (D-07).
- Output validation per D-08/D-10/D-11 with full agent-facing structured errors.
- Run lifecycle FSM (D-12) with a transition-guarded update API on `StateStore` so Phase 5/6 cannot regress monotonicity (closes 02-REVIEW MR-01 in the natural place).

**This Phase does NOT deliver:**
- Phase-4 `ctx.evm.*` (CTX-01..09) or `ctx.actions.*` builders for non-noop actions.
- Simulation, policy evaluation, signer integration (Phase 5/6).
- Promise / async strategies. Strategies are synchronous transformations only (D-10).
- Action variants beyond `Noop` — `executor-core::schema::action::Action` retains the existing single variant (Phase 4 extends it).
- Pooled JS runtimes — every run gets a fresh `Runtime + Context::base` (researcher Q6, locked).

When Phase 3 ships: agent calls `strategy_run { strategy_id }`, receives a `StrategyRunResponse` carrying `run_id` plus `outcome: { kind: "noop" | "actions" | "error" }`, and the run row + journal entries are observable via `execution_get` / `journal://{run_id}` (Phase-4+ extends).
</domain>

<decisions>
## Locked Decisions

### Sandbox engine

- **D-01: `rquickjs = "0.11"` with `default-features = false`.**
  - **Why:** AGENTS.md line 22 — "rquickjs for sandboxed JavaScript" is non-negotiable. Researcher 03-RESEARCH.md verified version 0.11.0 stable, MIT-licensed, sandbox-by-default (`Context::base` ships zero host APIs), AWS LLRT references give it production track-record.
  - **Disabled features (must not be enabled):** `futures` (we're synchronous), `loader` (would let JS resolve external modules), `dyn-load` (would let JS load `.so`/`.dll`), `parallel` (we serialise execution behind the storage Mutex). Plan 03-01 verifies via `cargo tree -p strategy-js | grep -E '(libloading|tokio)'` returning empty.
  - **Allocator:** default Rust global. Do NOT swap in mimalloc/jemalloc at the rquickjs level — `set_memory_limit` becomes a no-op with custom allocators (RESEARCH Pitfall 4).
  - **Alternatives rejected:** quickjs-rusty (parallel option, no AGENTS alignment), boa_engine (no sandbox model per maintainers), deno_core / v8 (heavyweight, GLIBC-pinned prebuilt, minutes-long first build).

### Crate placement

- **D-02: New workspace member `crates/strategy-js/`.**
  - **Why:** AGENTS.md line 33 explicitly lists `strategy-js/` in the target crate boundaries. The researcher's PATTERNS.md uses `executor-runtime` in some sections — translate every such reference to `strategy-js` while keeping all the analog conventions (Cargo.toml/lib.rs/error.rs scaffolding, Phase-2 mirroring, etc.). Cost is near-zero and matches documented architecture; staging inside `executor-mcp` would require a future move.
  - **Workspace `members`:** root `Cargo.toml` adds `crates/strategy-js` to the existing list `["crates/executor-mcp", "crates/executor-core", "crates/executor-state", "crates/executor-signer"]`.
  - **Per-crate deps only.** `rquickjs` is NOT promoted to `[workspace.dependencies]` — Phase 2 explicitly chose per-crate-only pinning until ≥2 crates consume the same dep (mirrors `executor-state/Cargo.toml:10-13` comment block). Plan 03-01 includes the same justifying comment.

### Resource defaults

- **D-03: Heap 64 MiB, GC threshold 8 MiB, stack 1 MiB, wall-clock 2 s.**
  - **Heap = 64 MiB.** QuickJS itself uses a few MiB; 64 MiB leaves room for strategies that build modest in-memory state (parsed ABIs in Phase 4+) without enabling pathological allocators. Smaller (16 MiB) would surprise legitimate uses; larger (256 MiB+) defeats the cap.
  - **GC threshold = 8 MiB** (1/8 of heap). Standard rquickjs guidance.
  - **Stack = 1 MiB** (rquickjs default is 256 KiB). Prevents C-stack overflow attacks while accommodating modest recursion. RESEARCH Pitfall 5 verified.
  - **Wall-clock = 2 s.** v1 strategies are validators / EVM-read-decision logic. 2 s of pure JS computation is generous; tunable via crate-level config in a future phase but **constants in v1** to avoid premature config surface.
  - **Sentinel handling:** `set_memory_limit(0)` means UNLIMITED in rquickjs (RESEARCH Pitfall 3). Use plain `usize` constants — never `Option::<usize>::unwrap_or(0)`. Plan 03-01 acceptance asserts `MEMORY_LIMIT_BYTES != 0`.

### `ctx` host surface (Phase 3 minimal)

- **D-04: Phase-3 `ctx` exposes ONLY the following members.** All other globals — `console`, `fetch`, `setTimeout`, `setInterval`, `setImmediate`, `queueMicrotask`, `XMLHttpRequest`, `WebSocket`, `require`, dynamic `import()`, `process`, `Worker`, `child_process`, `fs`, any `Deno.*`, any `node:*` — are absent (D-11).

  | Member | Type | Phase-3 behaviour |
  |--------|------|-------------------|
  | `ctx.strategy.id` | string (read-only, 64 hex) | injected at run start from `strategies.id` |
  | `ctx.strategy.name` | string (read-only) | injected from `strategies.name` |
  | `ctx.run.id` | string (read-only, ULID) | injected after `insert_run` returns |
  | `ctx.now()` | function → number | wraps `chrono::Utc::now().timestamp_millis() as f64` |
  | `ctx.log(...args)` | function → undefined | concats args with single-space separator into one string; **buffers** in a host-side `Vec<String>` (RESEARCH Pitfall 2, Q7) — never writes DB inside JS execution |
  | `ctx.actions.noop()` | function → string | returns the literal `"noop"` (convenience; equivalent to a top-level `"noop"` return) |

  **Deferred to Phase 4:** `ctx.evm.*`, `ctx.actions.contractCall`/`rawCall`/`erc20*`/`nativeTransfer`, `ctx.units`, address helpers (CTX-01..09).

  **`eval` and `Function` constructor caveat:** `Context::base` includes both as ECMAScript intrinsics. They do not grant host access; compiled JS runs under the same memory/wall-clock/intrinsic limits, so this is **not** a sandbox escape. We do not attempt to remove them.

  **Clock injection:** v1 uses `chrono::Utc::now()` directly. Deterministic clock (`Arc<dyn Fn() -> i64>`-style injection) deferred to a later phase if test-determinism need arises.

### Strategy entry-point shape

- **D-05: Shape B — strategy source MUST evaluate to a function `(ctx) => "noop" | Action[]`.**
  - **Why locked over Shape A (top-level expression / `globalThis.strategy = ...` allowed):** Shape B is a stricter contract; agents have one canonical authoring style; ambiguous source forms (top-level returns, named function with side effects on globalThis) are rejected with a clear `STRATEGY_INVALID_OUTPUT` error. Researcher's open-question Q1 explicitly flagged Shape B as "stronger".
  - **Wrapping:** runtime evaluates `(SOURCE)(__ctx)` where `__ctx` is the host-injected `ctx` global. The exact wrapping pattern is documented in Plan 03-01 Task 2.
  - **Validation (D-10):** if `(SOURCE)` evaluation does not produce a callable, OR the call return value is anything other than `"noop"` / `Action[]`, the run fails with `STRATEGY_INVALID_OUTPUT`. Promises returned from the function are explicitly rejected (D-10).

### Journal table layout

- **D-06: Three separate tables — `journal_source_reads`, `journal_actions`, `journal_logs`.**
  - **Why locked over a single polymorphic `journal_entries` table:** "what the strategy decided" (actions, noop, validation_error, runtime_error) and "what the strategy said" (logs) are different concepts with different read-access patterns. Forcing them into one `outcome` column makes "list the actions for this run" require `WHERE outcome IN (...)` filtering forever. Researcher recommended this in 03-RESEARCH.md "Why one combined table" comparison.
  - **Schema** — appended to `executor-state::schema::SCHEMA_SQL` (Phase 2 D-03b idempotent CREATE TABLE pattern; no migration crate, no `schema_version` table):

    ```sql
    CREATE TABLE IF NOT EXISTS journal_source_reads (
        id           TEXT PRIMARY KEY,           -- ULID per row
        run_id       TEXT NOT NULL REFERENCES runs(id),
        kind         TEXT NOT NULL,              -- 'strategy_source' in Phase 3; 'evm_call'/'erc20_balance'/etc in Phase 4
        target       TEXT NOT NULL,              -- strategy_id (hex) in Phase 3; address+selector etc in Phase 4
        payload_json TEXT,                       -- NULL in Phase 3; structured args in Phase 4+
        recorded_at  TEXT NOT NULL               -- RFC3339 UTC
    );
    CREATE INDEX IF NOT EXISTS idx_journal_source_reads_run_id
        ON journal_source_reads(run_id);

    CREATE TABLE IF NOT EXISTS journal_actions (
        id           TEXT PRIMARY KEY,           -- ULID per row
        run_id       TEXT NOT NULL REFERENCES runs(id),
        outcome      TEXT NOT NULL,              -- enum: 'noop'|'actions'|'validation_error'|'runtime_error'  (future-locked: 'simulation_failure'|'policy_denied' reserved at Phase 5)
        payload_json TEXT NOT NULL,              -- the validated Action[] JSON, or the error detail
        recorded_at  TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_journal_actions_run_id
        ON journal_actions(run_id);

    CREATE TABLE IF NOT EXISTS journal_logs (
        id           TEXT PRIMARY KEY,           -- ULID per row
        run_id       TEXT NOT NULL REFERENCES runs(id),
        message      TEXT NOT NULL,              -- single-string concatenation of ctx.log args
        recorded_at  TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_journal_logs_run_id
        ON journal_logs(run_id);
    ```

  - **`outcome` enum future-lock (mirrors Phase-2 D-05 RunStatus pattern):** Plan 03-02 introduces `pub enum JournalActionOutcome { Noop, Actions, ValidationError, RuntimeError, SimulationFailure /* Phase 5 */, PolicyDenied /* Phase 5 */ }` with a `phase3_emittable()` gate (only the first four). Schema golden locks all six wire names at Phase-3 introduction so Phase 5 cannot trigger contract churn. Phase-3 production code paths MUST NOT emit `simulation_failure` or `policy_denied`.

### MCP error codes

- **D-07: New MCP error codes (extend Phase-2 typed-error envelope).**

  Phase 2 used `-32014/-32015/-32016` (storage) and `-32602` (invalid_params). Phase 3 adds three:

  | Code | Constant | When emitted | `data.code` string |
  |------|----------|--------------|---------------------|
  | `-32011` | `STRATEGY_DELETED` | `strategy_run` invoked against a soft-deleted strategy (D-02c carry-over from Phase 2) | `"strategy_deleted"` |
  | `-32017` | `STRATEGY_RUNTIME_ERROR` | JS exception, OOM, wall-clock timeout, stack overflow | `"strategy_runtime_error"` (with `data.kind ∈ {"exception","oom","timeout","stack_overflow"}`) |
  | `-32018` | `STRATEGY_INVALID_OUTPUT` | Return value isn't `"noop"` / `Action[]`, OR strategy is not Shape B (D-05), OR strategy returned a Promise (D-10) | `"strategy_invalid_output"` |

  - **Locked.** Researcher 03-RESEARCH.md verified `-32011`, `-32017`, `-32018` unused by Phase 1/2. Plan 03-01 acceptance MUST run the same `grep` audit Phase 2 used (02-02 SUMMARY:213-215): `grep -r 'ErrorCode(-3201[178])' ~/.cargo/registry/src/index.crates.io-*/rmcp-1.5.0/` returns no hits. If the audit fails, the orchestrator/user is consulted before renumbering.
  - **`-32012`, `-32013`, `-32019` reserved** for future Phase 3/4 needs (e.g., distinct timeout code if `data.kind="timeout"` proves insufficient at the MCP level).
  - **Why three codes (not one giant `STRATEGY_FAILED`):** Agent UX. `data.code = "strategy_invalid_output"` tells the agent to fix the source; `data.code = "strategy_runtime_error" / data.kind = "timeout"` tells the agent to optimise; `data.code = "strategy_deleted"` tells the agent to re-register. Lumping them into one code forces every agent to parse free-form messages.

### `strategy_run` tool contract

- **D-08: `strategy_run` MCP tool.**
  - **Tool name:** **`strategy_run`** (NOT `strategy_run_once`). The Phase-1 placeholder is `strategy_run_once`; **Plan 03-03 renames the registered tool to `strategy_run`** and the input/response types are `StrategyRunInput` / `StrategyRunResponse`. The `_once` qualifier was a Phase-1 placeholder hint that lost its meaning once the tool became real (every Phase-3 invocation is one-shot — there is no `strategy_run_loop`). The renamed tool replaces the unimplemented_err placeholder.
  - **Note on inherited names:** the existing `executor-core::schema::strategy::StrategyRunOnceInput` is **kept as a deprecated alias** (re-exported `pub use StrategyRunInput as StrategyRunOnceInput;` in `executor-core::schema::strategy` for one phase) so existing tests / agents that still reference the Phase-1 name continue to compile during the transition. Phase 4 may delete the alias.

  - **Input** (added to `executor-core::schema::strategy`):

    ```rust
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    #[schemars(description = "Execute a registered JavaScript strategy once in a sandbox.")]
    pub struct StrategyRunInput {
        #[schemars(description = "Strategy id (lower-case hex SHA-256, 64 chars).")]
        pub strategy_id: String,
    }
    ```

  - **Output** (added to `executor-core::schema::execution`):

    ```rust
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum StrategyOutcome {
        Noop,
        Actions { actions: Vec<Action> },
        // validation_error / runtime_error / strategy_deleted are MCP errors, not response variants.
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    #[schemars(description = "Response for strategy_run (Phase 3).")]
    pub struct StrategyRunResponse {
        pub run_id: String,
        pub strategy_id: String,
        pub status: RunStatus,            // 'succeeded' or 'failed' (Phase 3 runs are synchronous to completion)
        pub started_at: String,
        pub finished_at: String,          // always populated — Phase-3 returns only on terminal status
        pub outcome: StrategyOutcome,
    }
    ```

    Note that `validation_error` / `runtime_error` / `strategy_deleted` are surfaced as **MCP error responses** (not success-with-error fields). Even when the run row exists with status `Failed` and the journal records the failure, the tool result is the error envelope. Agents reconstruct partial state via `execution_get { execution_id: run_id }` (Phase 2 — already wired) and `journal://{run_id}` (Phase 3 — Plan 03-03 wires).

  - **Schema goldens added in Plan 03-03:** `StrategyRunInput.json` (regenerated — was a Phase-1 placeholder), `StrategyRunResponse.json` (new), `StrategyOutcome.json` (new), `JournalActionOutcome.json` (new — locks all six future-reserved variants per D-06 future-lock).

- **D-08a: stdio integration tests (live in `crates/executor-mcp/tests/stdio_handshake.rs`).** Each test case below is an `#[tokio::test]` written by Plan 03-03. Naming mirrors Phase-2 D-08a verb-first / subject-first conventions.

  | Test name | Asserts |
  |-----------|---------|
  | `strategy_run_returns_noop_for_minimal_strategy` | source `(ctx) => "noop"` → `outcome.kind == "noop"`, `status == "succeeded"`, run row + 1 journal_actions row + 1 journal_source_reads row |
  | `strategy_run_returns_actions_for_action_array_strategy` | source `(ctx) => [{kind:"noop"}]` → `outcome.kind == "actions"`, `outcome.actions.length == 1`, `actions[0].kind == "noop"` |
  | `strategy_run_returns_actions_for_empty_array` | source `(ctx) => []` → `outcome.kind == "actions"`, `outcome.actions.length == 0` (semantically noop, wire form actions) |
  | `strategy_run_rejects_number_return` | source `(ctx) => 42` → MCP error -32018, `data.code == "strategy_invalid_output"`, `data.detail` mentions "number" / wrong shape |
  | `strategy_run_rejects_object_return` | source `(ctx) => ({foo: 1})` → MCP error -32018 |
  | `strategy_run_rejects_null_return` | source `(ctx) => null` → MCP error -32018 |
  | `strategy_run_rejects_promise_return` | source `(ctx) => Promise.resolve("noop")` → MCP error -32018, `data.detail` contains "promise" (D-10) |
  | `strategy_run_rejects_non_function_source` | source `"noop"` (top-level expression, NOT Shape B) → MCP error -32018, `data.detail` contains "function" / "(ctx) =>" hint (D-05) |
  | `strategy_run_rejects_phase4_action_kind` | source `(ctx) => [{kind:"contract_call"}]` → MCP error -32018 (Action enum has only Noop in Phase 3) |
  | `strategy_run_runtime_error_on_throw` | source `(ctx) => { throw new Error("nope"); }` → MCP error -32017, `data.kind == "exception"`, `data.detail` contains "nope" |
  | `strategy_run_runtime_error_on_infinite_loop` | source `(ctx) => { while(true){} }` → MCP error -32017, `data.kind == "timeout"` (wall-clock fires) |
  | `strategy_run_runtime_error_on_oom` | source `(ctx) => { let a=[]; while(true) a.push(new Array(1e6)); }` → MCP error -32017, `data.kind == "oom"` |
  | `strategy_run_runtime_error_on_stack_overflow` | source `(ctx) => { function f(){f();} f(); }` → MCP error -32017, `data.kind == "stack_overflow"` |
  | `strategy_run_rejects_deleted_strategy` | register strategy → soft-delete → run → MCP error -32011, `data.code == "strategy_deleted"` |
  | `strategy_run_records_source_read_journal_row` | After successful run, the new `journal_source_reads` table has exactly one row for that run with `kind="strategy_source"`, `target=<strategy_id>` |
  | `strategy_run_records_log_messages` | source `(ctx) => { ctx.log("hello", 42); ctx.log("world"); return "noop"; }` → after run, `journal_logs` has 2 rows with messages `"hello 42"` and `"world"` |
  | `strategy_run_run_row_status_transitions_to_failed_on_error` | After any -32017/-32018 error, the corresponding run row exists with status `failed` (observable via `execution_get`) |
  | `strategy_run_invalid_strategy_id_format_returns_invalid_params` | `strategy_id = "ZZZ"` → MCP error -32602 (D-09 — does not even reach storage) |
  | `strategy_run_unknown_strategy_id_returns_not_found` | well-formed but absent id → MCP error -32014 (storage layer) |

  Total: **19 stdio test cases**, all created in Plan 03-03 Task 3. Each test spawns its own server via the existing `common::spawn_server_with_state(db_path)` helper.

  Plan 03-03 also updates the existing Phase-1 `unimplemented_tools_return_phase_hint` test to drop `strategy_run_once` from its case list (now implemented) and updates `policy_update` to be the only tool that still returns -32010 in that test.

### Validation rules for `strategy_run` params

- **D-09: `strategy_run` parameter validation** (mirrors Phase-2 D-09a regex pattern, lives in `executor-mcp::validation`).
  - **`strategy_id` format**: 64 lowercase hex chars (regex `^[0-9a-f]{64}$`). Reuses the existing `validation::validate_strategy_id_format` helper from Phase 2 (already in tree at `crates/executor-mcp/src/validation.rs`).
  - **No additional bounds for Phase 3** — there are no other parameters; the strategy source itself was bounded at register-time (D-09 256 KiB cap from Phase 2).
  - **Failure path:** `validate_strategy_id_format(&input.strategy_id).map_err(invalid_params)?` — same shape as `strategy_delete` (Phase 2 02-02 tools.rs:159).

### Async / promise handling

- **D-10: Promises returned from the strategy entry-point are STRATEGY_INVALID_OUTPUT.**
  - **Why:** Phase-3 strategies are synchronous transformations (no `futures` feature; `Context::eval` returns synchronously). The Promise constructor is still an ECMAScript intrinsic so user code *can* construct one — but if returned, validation rejects it with `data.detail = "promise return values are not supported in v1; strategies must be synchronous"`.
  - **Detection:** The runtime checks the `rquickjs::Value` variant after invoking the entry function. If it's a `Promise`-like object (object with `.then` callable), reject before JSON conversion. Plan 03-01 acceptance includes a unit test in `strategy-js`.
  - **Future direction:** v2 may add Promise resolution at the host boundary if EVM-async helpers (Phase 4+) require it. Out of scope for Phase 3.

### Forbidden host capability regression tests

- **D-11: One regression test per absent global.** Each of these MUST cause an `InvalidOutput` error or be `=== undefined` inside the sandbox:
  - `console`, `console.log`
  - `setTimeout`, `setInterval`, `setImmediate`, `queueMicrotask`
  - `fetch`, `XMLHttpRequest`, `WebSocket`
  - `require`, dynamic `import()` (returns rejected promise OR throws — either is acceptable)
  - `process`, `globalThis.process`
  - `Worker`, `child_process`
  - `fs`, any `node:fs` import
  - `Deno.readFile`, any `Deno.*`

  **Implementation:** Plan 03-01 unit tests in `strategy-js` execute strategies of the form `(ctx) => { return typeof console === "undefined" ? "noop" : "BAD"; }` for each name and assert the result is `"noop"`. A consolidated test `sandbox_blocks_host_globals` enumerates the full list. (`require` and dynamic `import()` get bespoke tests because their failure modes differ — runtime exception vs rejected promise.)

  **Pitfall (RESEARCH 1):** `Context::base` excludes module/import/require intrinsics by default, but `Context::full` would include them. We use `Context::base`. Plan 03-01 acceptance asserts the `base` constructor (not `full`) is used.

### Run lifecycle FSM (closes 02-REVIEW MR-01)

- **D-12: Run lifecycle is `Queued → Running → Succeeded | Failed`.**
  - The Phase-2 `RunStatus` enum already declared all 7 variants with `phase2_emittable()` gating. Phase 3 emits only `Queued`, `Running`, `Succeeded`, `Failed` — same set as `phase2_emittable`. **No enum change needed.**
  - **MR-01 fix (transition guard):** Plan 03-02 adds a new `StateStore::update_run_status_with_transition(run_id, from: RunStatus, to: RunStatus) -> Result<(), StateError>` method that performs the update inside a single SQL `UPDATE runs SET status = ?to WHERE id = ?id AND status = ?from` and returns `StateError::InvalidInput("run {id} not in expected state {from:?}")` if `affected == 0` despite the row existing. This closes the silent-overwrite race that 02-REVIEW flagged. The existing `update_run_status` is **kept** for backwards compatibility (used by Phase 5/6 simulation/policy-failure transitions), but Phase 3's `strategy_run` handler MUST use the transition-guarded variant for every status change.
  - **Allowed transitions** (enforced at the handler level via the new API):
    - `Queued → Running` (immediately after row insert, before JS execution)
    - `Running → Succeeded` (on validated `Action[]` / noop)
    - `Running → Failed` (on runtime error or invalid output)
    - `Queued → Failed` (if strategy is soft-deleted between `register` and `run` — short window inside the same handler invocation; the handler short-circuits with `STRATEGY_DELETED` before inserting the run, so this transition is rare. If it occurs (e.g., concurrent delete via `strategy_delete`), the handler still inserts a run row and immediately transitions to Failed for journal completeness.)
  - **Disallowed (and cannot fire because of the guard):** `Succeeded → *`, `Failed → *`, `Running → Running`, `Running → Queued`, etc. Plan 03-02 adds a unit test `update_run_status_with_transition_rejects_unexpected_from` proving this.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before implementing:**

### Project planning
- `.planning/PROJECT.md` §Constraints (sandbox isolation, observability), §Out of Scope (no DSL / TypeScript / scheduler in v1)
- `.planning/REQUIREMENTS.md` STR-03/04/05, STJ-03/04 (verbatim text quoted in each plan)
- `.planning/ROADMAP.md` §"Phase 3: JavaScript Strategy Runner" (3-plan split, success criteria)

### Phase 3 artefacts
- `03-RESEARCH.md` — rquickjs API surface, resource limits, ctx host pattern, error code rationale, Pitfalls 1–14
- `03-PATTERNS.md` — file-level analogs (mirror Phase 2 conventions; rename `executor-runtime` → `strategy-js` everywhere)
- `03-VALIDATION.md` — per-task automated verify map (sibling file)

### Prior phase artefacts (mirror conventions)
- `.planning/phases/02-strategy-state-and-journal/02-CONTEXT.md` — D-XX numbering style, error-code reservations
- `.planning/phases/02-strategy-state-and-journal/02-01-PLAN.md`, `02-02-PLAN.md`, `02-03-PLAN.md` — task / verification / acceptance shape
- `.planning/phases/02-strategy-state-and-journal/02-01-SUMMARY.md`..`02-03-SUMMARY.md` — what was actually delivered

### External
- rquickjs 0.11 docs.rs: `Runtime::set_memory_limit`, `set_max_stack_size`, `set_interrupt_handler`, `set_gc_threshold`, `Context::base`, `Context::with`
- AGENTS.md §Technology Stack (line 22 — `rquickjs`), §Architecture (line 33 — `strategy-js/`)
- ULID spec, RFC3339 — already used in Phase 2; no change

</canonical_refs>

<code_context>
## Existing Code Insights (verified in tree)

### Reusable assets (do not re-create)
- `crates/executor-state/src/{schema.rs,store.rs,strategies.rs,runs.rs,error.rs}` — Phase 2 patterns. Plan 03-02 extends `schema.rs` (append three CREATE TABLE blocks), adds `journal.rs` module, extends `store.rs` façade, extends `error.rs` only if a journal-specific failure mode is genuinely distinct (otherwise reuse `Storage`/`NotFound`/`InvalidInput`).
- `crates/executor-mcp/src/{errors.rs,validation.rs,tools.rs,resources.rs,server.rs}` — Phase 2 wiring lives here. Plan 03-03 extends each:
  - `errors.rs`: append three `pub const` codes + `map_runtime_error(RuntimeError) -> McpError` + `strategy_deleted(id)` + `strategy_runtime_error(kind, msg, run_id)` + `strategy_invalid_output(detail, run_id)` helpers (RESEARCH structured `data` payload pattern).
  - `validation.rs`: no new code — reuses `validate_strategy_id_format` for the new tool.
  - `tools.rs`: replace the placeholder `strategy_run_once` body with the real `strategy_run` handler (and rename — see D-08).
  - `resources.rs`: replace the phase-gated `journal://` branch with a real reader hitting the new tables; mirror `read_strategy` structure (resources.rs:121-167).
  - `server.rs`: add `runner: Arc<strategy_js::Sandbox>` field next to `state` (or construct per-call inside the handler — Plan 03-03 confirms which after Wave 0 measures `Runtime::new()` cost; if < 50 ms, per-call construction wins for isolation; if higher, pool of 1 behind a Mutex).
- `crates/executor-mcp/tests/common/mod.rs` — `spawn_server_with_state`, `call_tool`, `extract_json_result`, `initialize` already exist and Plan 03-03's stdio tests use them directly.
- `crates/executor-core/src/schema/{strategy.rs,execution.rs,action.rs}` — Phase 1+2 schemas. Plan 03-03 extends `execution.rs` (`StrategyRunResponse` + `StrategyOutcome`) and `strategy.rs` (`StrategyRunInput` + the deprecated alias `StrategyRunOnceInput`). The `Action::Noop` variant in `action.rs` is **not** changed (Phase 4 owns variant additions per CTX-05..08).

### Established patterns
- **Mutex placement (Pitfall 4 carry-over):** `StateStore` owns a bare `Connection`; outer `Arc<tokio::sync::Mutex<StateStore>>` lives in `executor-mcp::ExecutorServer`. Every DB call goes through `tokio::task::spawn_blocking { let mut store = state.blocking_lock(); store.<call>() }`. Tokio mutex never held across `await`. Plan 03-03's `strategy_run` follows this for ALL of: get_strategy_by_id, insert_run, update_run_status_with_transition, record_source_read, record_action_outcome, record_log. The JS execution itself is ALSO inside `spawn_blocking` (rquickjs `Runtime` is `!Sync` — see RESEARCH Concurrency Plan).
- **Phase-2 typed error envelope:** `McpError::new(code, message, Some(json!({"code": "<snake>", ...})))`. Same shape across `unimplemented_err`, `map_state_error`, `invalid_params`, `storage_error`. Plan 03-03 mirrors this for all three new error helpers.
- **Future-reserved enum gates (D-05 carry-over):** Run status declared all 7 variants at Phase 2 with `phase2_emittable`. Same pattern for `JournalActionOutcome` in Plan 03-02 (declare all six, gate to `phase3_emittable`).
- **Per-crate dep pinning:** `rquickjs` declared only in `crates/strategy-js/Cargo.toml`; not promoted to `[workspace.dependencies]` (mirrors `executor-state/Cargo.toml:10-13` comment block).

### Integration points
- `ExecutorServer::new(&StateConfig)` (executor-mcp/src/server.rs:44-55) — Plan 03-03 extends to also construct or wire the `strategy-js::Sandbox`. Constructor remains `anyhow::Result<Self>`; Default impl stays removed (Phase 2 D-removed-default precedent).
- `tools::strategy_run_once` (executor-mcp/src/tools.rs:215-224) — currently returns `unimplemented_err("strategy_run_once", 6)`. Plan 03-03 replaces with real `strategy_run` handler and renames the registered tool name.

</code_context>

<deferred>
## Deferred Ideas (DO NOT plan in Phase 3)

- Phase-4 surface: `ctx.evm.readContract`, ERC20 helpers, action builders for ContractCall/RawCall/Erc20Approve/Erc20Transfer/NativeTransfer, units / address helpers (CTX-01..09).
- Simulation, policy evaluation, signer integration (Phase 5/6).
- TypeScript transpilation (V2-01).
- Async strategies / Promise-returning strategies — explicitly rejected for v1 (D-10). v2 may revisit if EVM-async helpers require host-side resolution.
- Streaming / chunked output — strategy returns a single value.
- Multi-strategy parallel execution — v1 serialises run requests behind the storage Mutex.
- Persistent VM caching — every run gets a fresh `Runtime + Context` (researcher Q6, locked).
- Configurable resource limits at runtime — D-03 constants are baked in for v1.
- Pooled runtimes — defer until Wave-0 measurement shows `Runtime::new()` cost > 50 ms (RESEARCH Q6).
- `console` aliasing to `ctx.log` — keep `console` undefined; force `ctx.log` (RESEARCH ctx host API rationale).
- `schema_version` migration table — defer until a destructive schema change is required (Phase-2 D-03b precedent).
- Cross-strategy journal queries (e.g., "show me all runtime errors in the last 24h") — Phase 3 only delivers per-run journal access via `journal://{run_id}`. Aggregations are out of scope.

</deferred>

---

*Phase: 03-javascript-strategy-runner*
*Context locked: 2026-04-27 (planner-locked from 03-RESEARCH.md after /gsd-discuss-phase was skipped)*
