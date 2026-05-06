---
phase: 03
phase_name: javascript-strategy-runner
requirements: [STR-03, STR-04, STR-05, STJ-03, STJ-04]
status: complete
researched: 2026-04-27
domain: sandboxed JavaScript execution + journal extension
confidence: HIGH
---

# Phase 3: JavaScript Strategy Runner — Research

## Phase Goal Recap

Phase 3 wires `strategy_run_once` from a `-32010` placeholder into a real, sandboxed JavaScript execution path. After this phase:

1. The MCP tool `strategy_run_once { strategy_id }` loads a strategy from `executor-state`, runs its source in a QuickJS sandbox under hard resource limits, and emits a run row whose status transitions `Queued → Running → Succeeded | Failed`.
2. The sandbox blocks every host capability the strategy could otherwise reach — filesystem, network, process, timers, dynamic native modules.
3. The runtime journals the source-read event (STJ-03) and the validated `Action[]`/`noop` return value or its validation error (STJ-04). Two new tables are introduced: `journal_source_reads` and `journal_actions`.
4. Return shapes that are not `Action[]` or the literal string `"noop"` are rejected with structured MCP errors using the Phase 2 error-code conventions plus three new Phase 3 codes.

The phase explicitly does **not** populate `Action` variants beyond `Noop` (Phase 4 owns CTX-01..09), does **not** simulate or sign (Phase 5/6), and does **not** introduce policy decisions (Phase 5).

**Primary recommendation:** Use `rquickjs 0.11` with `Context::base` + a hand-curated globals set. Run JS execution under `tokio::task::spawn_blocking` (matches the Phase 2 storage pattern). Enforce wall-clock with `Runtime::set_interrupt_handler`, memory with `Runtime::set_memory_limit`, and stack with `Runtime::set_max_stack_size`. Do **not** enable the `futures`, `loader`, or `dyn-load` features.

---

<user_constraints>
## User Constraints (from CONTEXT.md)

No `03-CONTEXT.md` exists yet — Phase 3 has not run through `/gsd-discuss-phase`. The orchestrator's objective declares the locked decisions inline:

### Locked Decisions (from objective brief + AGENTS.md / PROJECT.md)

- **JS engine:** rquickjs (AGENTS.md §Technology Stack — "rquickjs for sandboxed JavaScript"). [VERIFIED: AGENTS.md line 22]
- **Sandbox isolation contract** is non-negotiable per PROJECT.md §Constraints — strategy code MUST NOT access private keys, filesystem, arbitrary network, process API, or direct RPC clients. [VERIFIED: PROJECT.md lines 77, 60–61 of AGENTS.md]
- **Return shape:** `Action[]` or `noop`. Strategy never signs, never broadcasts. [VERIFIED: PROJECT.md, REQUIREMENTS.md STR-05]
- **`ctx` API stays minimal in Phase 3** — Phase 4 owns `ctx.evm.*` and action builders (CTX-01..09). Phase 3 must not bleed Phase 4 surface in.
- **Concurrency:** v1 single-operator runtime. Match Phase 2's `Arc<Mutex<StateStore>>` + `spawn_blocking` pattern. [VERIFIED: 02-CONTEXT.md D-03d, server.rs lines 30–55]
- **Error model:** extend the Phase 2 typed-error envelope (`-32014` not_found, `-32015` name_conflict, `-32016` storage_error, `-32602` invalid_params). New codes go in the unused `-32011..-32013` and `-32017..-32019` band of the JSON-RPC server-defined range. [VERIFIED: errors.rs lines 24–33]

### Claude's Discretion

- Specific rquickjs feature flags to enable / disable (default-features = false?).
- Exact wall-clock and heap limit values (defaults proposed below).
- `ctx.now()` clock injection — `chrono::Utc::now()` vs a deterministic test clock.
- Whether to expose `console.log` as a no-op, a journal sink, or omit entirely (recommendation: journal sink).
- New journal-table column names within Phase 2's RFC3339 / ULID conventions.
- Whether to introduce a `strategy-js` crate now (per AGENTS.md target architecture) or stage it inside `executor-mcp` and split later.

### Deferred Ideas (OUT OF SCOPE — do not research)

- Phase 4 surface (`ctx.evm.readContract`, ERC20 helpers, action builders for ContractCall/RawCall/Erc20*/NativeTransfer).
- Simulation, policy evaluation, signer integration (Phase 5/6).
- TypeScript transpilation (V2-01).
- Async strategies / Promise-returning strategies — v1 strategies are sync function-style returns. Promise resolution at the host boundary is **deferred** unless required to run the QuickJS engine itself (it is not — `Context::eval` returns synchronously).
- Streaming / chunked output. Strategy returns a single value.
- Multi-strategy parallel execution. v1 serialises run requests behind the same `Mutex`.
- Persistent VM caching — every run gets a fresh `Runtime` + `Context`.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description (verbatim from REQUIREMENTS.md) | Research Support |
|----|-----|-----|
| **STR-03** | Runtime can execute a registered strategy with a sandboxed `ctx`. | rquickjs `Runtime` + `Context::base` + injected `ctx` global; execution path in `strategy_run_once` § *strategy_run Tool Wiring* |
| **STR-04** | Strategy code cannot access private keys, filesystem, process APIs, arbitrary network, or direct RPC clients. | `Context::base` rejects all of these by default (none are intrinsics); confirmed in *Sandbox Isolation Contract* table; enforcement via not-enabling `loader`/`dyn-load` features |
| **STR-05** | Strategy returns `Action[]` or `noop`, and runtime rejects unsupported return shapes. | JSON-bridged validation against `Vec<Action>` deserializer + literal-string check; § *Action[]/noop Output Schema* |
| **STJ-03** | Runtime records source reads performed during each run. | New `journal_source_reads` table; one row per run captures `(run_id, strategy_id, source_hash, read_at)`; § *Source-Read & Journal Capture Strategy* |
| **STJ-04** | Runtime records returned actions and validation errors. | New `journal_actions` table: `(run_id, outcome, payload_json, recorded_at)` where outcome ∈ `actions / noop / validation_error / runtime_error`; § *Source-Read & Journal Capture Strategy* |

**Success criteria (from ROADMAP):**

1. Agent can run a registered JS strategy once → covered by tool wiring + integration test `strategy_run_once_returns_noop_for_minimal_strategy`.
2. Forbidden host access is blocked → covered by tests asserting `typeof require === 'undefined'`, `typeof fs === 'undefined'`, `typeof fetch === 'undefined'`, `typeof process === 'undefined'`, `typeof setTimeout === 'undefined'`.
3. Source reads and returned actions/errors are journaled → covered by integration tests reading from new journal tables after a run.
4. Invalid return shapes are rejected with actionable MCP tool errors → covered by tests that submit strategies returning `42`, `null`, `{}`, throwing exceptions, or running past the wall-clock deadline.
</phase_requirements>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| QuickJS lifecycle (Runtime/Context construction, limits, intrinsic curation) | `strategy-js` crate (or staged `executor-mcp::strategy_runner` module) | — | Dedicated boundary so Phase 4 can extend `ctx` without touching MCP wiring |
| `ctx.now()` / `ctx.log()` host-function injection | same as above | — | Pure JS-engine concern, no DB or MCP dependency |
| Source-read journaling | `executor-state` (table + repo functions) | `executor-mcp` (calls into repo from `strategy_run_once`) | New table belongs with the rest of the persistence layer |
| Action-output validation | `executor-core::schema::action` (existing types) + new `validate_actions` helper in same module | `executor-mcp` (calls validator, surfaces error) | Schema crate already owns `Action` enum; validation is a pure function over JSON |
| Run lifecycle transitions (`queued → running → succeeded/failed`) | `executor-state::runs` (existing `update_run_status`) | `executor-mcp::tools::strategy_run_once` (drives the transitions) | Phase 2 already shipped `update_run_status` + `phase2_emittable` gate. No production-code change to that file |
| MCP error mapping for runtime/validation/timeout | `executor-mcp::errors` (extend with three new codes) | — | Mirrors Phase 2 pattern exactly |
| Wall-clock / memory / stack enforcement | `strategy-js` (interrupt handler closure inspecting `Instant`) | — | Lives at the QuickJS Runtime config site |

## JS Sandbox Recommendation

**Recommendation: `rquickjs 0.11.0`** (latest stable, published 2025-12-24, 1.69M downloads). [VERIFIED: crates.io API 2026-04-27]

### Comparison Matrix

| Crate | Version (verified) | Engine | Default sandbox posture | Memory limit | Wall-clock / interrupt | Build complexity | License | Verdict |
|-------|--------------------|--------|------------------------|--------------|----------------------|-----------------|---------|---------|
| **rquickjs** | 0.11.0 (2025-12-24) | QuickJS-NG (C, vendored) | "doesn't aim to provide system and web APIs" — no fs/net/timers/console without explicit injection | `Runtime::set_memory_limit(usize)` | `Runtime::set_interrupt_handler(closure)` polled regularly during interpretation; closure returns `true` → uncatchable exception | Bundled C source, no GLIBC pinning, builds on macOS/Linux/Windows out of the box | MIT | **CHOSEN** [CITED: docs.rs/rquickjs/latest/rquickjs/struct.Runtime.html, github.com/DelSkayn/rquickjs README] |
| quickjs-rusty | 0.11.1 (2026-03-21) | QuickJS (Bellard fork) | similar — no host APIs by default | yes (memory_limit) | yes (interrupt handler) | similar to rquickjs | MIT | viable alternate; rquickjs has bigger community + AWS LLRT track-record [VERIFIED: crates.io] |
| boa_engine | 0.21.1 (2026-03-29) | pure-Rust JS engine | "does not include any sandboxing as of right now" per maintainers; ECMAScript 94% conformance | not native (Rust allocator) | cooperative — Job queue, no built-in interrupt API | pure Rust, no C deps | Unlicense / MIT | **rejected** — no first-class instruction-budget primitive, sandbox not a stated goal yet [CITED: boajs.dev, x-cmd.com 251025 release notes] |
| deno_core | 0.399.0 (2026-04-22) | V8 | "secure by default" but sandbox is permissions-based, not isolate-based per-call | yes (V8 isolate options) | yes (V8 interrupt) | **HEAVY** — pulls in `v8` crate (147.4.0), GLIBC-pinned prebuilt binaries, ~minutes-long first build | MIT | **rejected** — far heavier than v1 needs; v1 is single sync evaluation per run, not a long-lived runtime |
| v8 (raw) | 147.4.0 (2026-04-24) | V8 | none by default | yes | yes | **HEAVIEST** — same prebuild GLIBC issue + lower-level API | MIT | **rejected** — same as deno_core, with more glue code |

### Why rquickjs

1. **Sandbox-by-default posture.** "rquickjs doesn't aim to provide system and web APIs" — that means `Context::base` and even `Context::full` ship **zero** host capabilities. fs, net, fetch, process, setTimeout, require, import — all absent. The embedder must explicitly add anything the strategy can call. This matches STR-04 perfectly: deny-by-default, allow-by-injection. [CITED: github.com/DelSkayn/rquickjs README via WebFetch 2026-04-27]
2. **Hard resource limits are first-class.** `set_memory_limit`, `set_max_stack_size`, `set_interrupt_handler` are documented `Runtime` methods. [VERIFIED: docs.rs/rquickjs/latest/rquickjs/struct.Runtime.html]
3. **Sync evaluation is the default.** `Context::with(|ctx| ctx.eval::<Value, _>(source))` returns synchronously. We don't need the `futures` feature in v1 — strategies are pure transformations. Avoiding `futures` reduces complexity and dependency surface.
4. **No C-build pain.** rquickjs vendors QuickJS-NG source. No system QuickJS install needed; no GLIBC pinning like the `v8` crate.
5. **Active maintenance.** Repo is the upstream for AWS Lambda's LLRT runtime — real production usage. [CITED: AWS LLRT mention in rquickjs README]
6. **MIT license** — workspace already uses Apache-2.0; MIT is compatible.

### Alternatives considered, rejected with reason

- **boa_engine**: maintainers explicitly state no sandbox model yet. Even though pure-Rust eliminates the C build chain, the lack of a documented instruction-budget / interrupt API forces the runtime to enforce timeouts via `tokio::time::timeout` racing the eval future, which leaves the interpreter in an undefined state on cancellation.
- **deno_core / v8**: orders of magnitude heavier dependency footprint and build time. Justified for hosting an entire programming environment; overkill for "run a 256 KiB pure JS function once and read its return value."
- **quickjs-rusty**: viable parallel option; rquickjs chosen for ecosystem reach and AGENTS.md alignment ("rquickjs" verbatim).

### Cargo dependency

```toml
# crates/strategy-js/Cargo.toml  (new crate, recommended)
[dependencies]
rquickjs = { version = "0.11", default-features = false }
# Do NOT enable: "futures" (we run sync), "loader" (would let JS resolve
# external modules), "dyn-load" (would let JS load .so/.dll), "parallel"
# (we serialise execution behind the storage Mutex).
```

[ASSUMED] Default features are minimal and don't pull in `loader`/`dyn-load`/`futures`. **Plan must verify** by running `cargo tree -p strategy-js` and asserting no `tokio` / `libloading` transitive dep through rquickjs.

## Resource Limit Patterns

Three independent budgets are enforced. Each maps to one rquickjs API.

### 1. Memory cap

```rust
let rt = rquickjs::Runtime::new()?;
rt.set_memory_limit(STRATEGY_HEAP_BYTES);   // recommend 64 MiB for v1
rt.set_gc_threshold(STRATEGY_GC_THRESHOLD); // recommend 8 MiB
```

When the engine attempts to allocate past the limit, it raises a JavaScript `OutOfMemory` error. From Rust this surfaces as `rquickjs::Error::OutOfMemory` (or a generic `Exception` carrying the message). The strategy run fails with status `Failed` and journal `outcome = runtime_error`.

**Default recommendation: 64 MiB heap, 8 MiB GC threshold.** Strategies in v1 are small validators — typical working set is single-digit MB. 64 MiB gives generous headroom while still capping pathological allocators.

### 2. Stack cap

```rust
rt.set_max_stack_size(STRATEGY_STACK_BYTES);  // recommend 1 MiB
```

Default rquickjs is 256 KiB. Recommend 1 MiB so deeply recursive strategies don't trip the limit prematurely while still preventing C-stack overflow attacks. [CITED: docs.rs/rquickjs/latest/rquickjs/struct.Runtime.html#method.set_max_stack_size]

### 3. Wall-clock deadline (the critical one)

```rust
let deadline = std::time::Instant::now() + STRATEGY_WALL_CLOCK;  // recommend 2s
rt.set_interrupt_handler(Some(Box::new(move || {
    std::time::Instant::now() >= deadline
})));
```

The interrupt handler is "regularly called by the engine when it is executing code"; returning `true` raises an **uncatchable** exception. [CITED: docs.rs/rquickjs/latest/rquickjs/struct.Runtime.html#method.set_interrupt_handler]

**Default recommendation: 2-second wall clock.** Tunable via config in a later phase; locked-in for v1.

**Pattern:**

```rust
fn run_strategy(source: &str, deadline_ms: u64) -> Result<JsValue, RunnerError> {
    let rt = rquickjs::Runtime::new()?;
    rt.set_memory_limit(64 * 1024 * 1024);
    rt.set_max_stack_size(1024 * 1024);
    rt.set_gc_threshold(8 * 1024 * 1024);

    let deadline = Instant::now() + Duration::from_millis(deadline_ms);
    rt.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));

    let ctx = rquickjs::Context::base(&rt)?;
    ctx.with(|c| {
        // 1. install ctx global (see ctx Host API)
        // 2. wrap the user source so we capture its return value:
        //    "(function(ctx){ <SOURCE>; return strategy(ctx); })(globalThis.__ctx)"
        //    OR require strategies to define `globalThis.strategy = function(ctx){...}`
        //    and invoke that. See "strategy entry-point shape" below.
        let result: rquickjs::Value = c.eval(WRAPPED_SOURCE)?;
        Ok(json_value_from_qjs(result))
    })
}
```

### Strategy entry-point shape (locked decision suggestion)

Two viable shapes — pick one. Recommendation: **shape A** (top-level expression with a default export-like convention). It avoids the user having to know about `globalThis`.

**Shape A (recommended):** Strategy source must end with a single expression that evaluates to either `"noop"` or `Action[]`, OR the source must define a function named `strategy` and the runtime calls `strategy(ctx)`. The runtime first tries `globalThis.strategy?.(ctx)`; if that's not a function, it evaluates the script as an expression and uses its completion value.

**Shape B:** require strategies to be `(ctx) => Action[] | "noop"` and the runtime evaluates `(SOURCE)(ctx)`. Cleaner contract but rejects strategies that prefer named-function style.

**Decision deferred to discuss-phase.** Both work; A is more forgiving, B is more rigid (good).

## ctx Host API (Phase 3 minimal surface)

Phase 3 keeps `ctx` deliberately tiny. Phase 4 will add `ctx.evm.*` and `ctx.actions.*`; Phase 3 only provides what STR-03 / STJ-03 / STJ-04 need to be testable.

| Member | Type | Purpose | Phase 3 behavior |
|--------|------|---------|------------------|
| `ctx.strategy.id` | string | content-addressed strategy id (hex sha256, 64 chars) | Read-only string injected at run start |
| `ctx.strategy.name` | string | human-readable name | Read-only |
| `ctx.run.id` | string | ULID of the current run | Read-only |
| `ctx.now()` | function → number | wall-clock millis since Unix epoch | Returns `chrono::Utc::now().timestamp_millis()` cast to JS Number. Deterministic in tests by injecting a fixed clock |
| `ctx.log(...args)` | function → undefined | journal-only logging — strings joined with spaces, written to a journal log table | Captures each call as a `journal_actions` row with `outcome = "log"` OR a separate `journal_logs` table — recommendation below |
| `ctx.actions.noop()` | function → string | returns the literal `"noop"` so authors don't have to type the magic string | Convenience; strategy may also return `"noop"` directly |

**Explicitly absent (and Phase 3 must include a regression test for each):**

- `console`, `console.log` — strategies must use `ctx.log` (this is by design — `console.log` would be untraced; `ctx.log` is journaled). [VERIFIED: rquickjs README — "Users need to manually insert a console object"]
- `setTimeout`, `setInterval`, `setImmediate`, `queueMicrotask` — not exposed.
- `fetch`, `XMLHttpRequest` — not exposed.
- `require`, `import()` (dynamic import) — module loader not enabled.
- `process`, `globalThis.process` — not exposed.
- `Function` constructor — **caveat below**.
- File / network / OS APIs of any kind — none exposed.

### `eval` and `Function` — important caveat

QuickJS's `Context::base` and `Context::full` both include the `Function` constructor and global `eval` because they are ECMAScript intrinsics. **These do not give the strategy any host access** — they only let the strategy compile more JS. Since that compiled JS is also subject to the same memory/wall-clock/intrinsic limits, this is not a sandbox escape. We do not attempt to remove them.

[ASSUMED — needs verification in Wave 0] That `Context::base` excludes any `module`/`import`/`require` intrinsic. The plan must include an integration test asserting `typeof require`, `typeof import`, and that no module specifier resolves.

### Recommendation: keep `console` undefined, force `ctx.log`

Tempting to alias `console.log → ctx.log` for ergonomics, but that creates two paths to the same journal row. Keep `console` undefined; document the convention. Test asserts `typeof console === 'undefined'`.

## Source-Read & Journal Capture Strategy

STJ-03 ("source reads") is broader than reading the strategy source itself — Phase 4 will add `ctx.evm.readContract`, ERC20 reads, etc., and each must journal. **Phase 3 only needs to journal the strategy source read.** Future phases extend the same table for EVM reads (with a different `kind` column value).

### New table: `journal_source_reads`

```sql
CREATE TABLE IF NOT EXISTS journal_source_reads (
    id           TEXT PRIMARY KEY,           -- ULID per read row
    run_id       TEXT NOT NULL REFERENCES runs(id),
    kind         TEXT NOT NULL,              -- 'strategy_source' in Phase 3; 'evm_call' / 'erc20_balance' / etc. in Phase 4
    target       TEXT NOT NULL,              -- strategy_id (hex) in Phase 3; address+selector in Phase 4
    payload_json TEXT,                       -- NULL in Phase 3; structured args in Phase 4
    recorded_at  TEXT NOT NULL               -- RFC3339 UTC
);
CREATE INDEX IF NOT EXISTS idx_journal_source_reads_run_id ON journal_source_reads(run_id);
```

**Phase 3 emits exactly one row per run** at the moment the runtime fetches the strategy source from `StateStore::get_strategy_by_id`, before evaluation begins. The `kind = "strategy_source"` value will never appear in Phase 4+ source reads.

### New table: `journal_actions`

```sql
CREATE TABLE IF NOT EXISTS journal_actions (
    id           TEXT PRIMARY KEY,           -- ULID
    run_id       TEXT NOT NULL REFERENCES runs(id),
    outcome      TEXT NOT NULL,              -- enum: 'noop' | 'actions' | 'validation_error' | 'runtime_error' | 'log'
    payload_json TEXT NOT NULL,              -- the validated Action[] JSON, or the error detail, or the log message
    recorded_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_journal_actions_run_id ON journal_actions(run_id);
```

The `outcome` enum is **future-locked at Phase 3** the same way `RunStatus` was future-locked at Phase 2 (D-05). Add `'simulation_failure'` and `'policy_denied'` reservations now if Phase 5's design is clear; otherwise leave the door open via a documented "extend at the introducing phase" comment.

**Phase 3 emits**:
- One `outcome = 'noop'` row when the strategy returns `"noop"`. `payload_json = '"noop"'`.
- One `outcome = 'actions'` row when the strategy returns a valid `Action[]` (in v1 this is `[{kind:"noop"}, ...]` only — Phase 4 adds variants). `payload_json = <JSON array>`.
- One `outcome = 'validation_error'` row when the return shape fails validation. `payload_json = { "code": "...", "detail": "..." }`.
- One `outcome = 'runtime_error'` row when the JS throws / hits memory / hits wall-clock. `payload_json = { "kind": "exception"|"oom"|"timeout"|"stack_overflow", "message": "..." }`.
- N `outcome = 'log'` rows, one per `ctx.log(...)` call. (Or a separate `journal_logs` table — see open question.)

### Why one combined table for actions+logs vs separate

| Option | Pros | Cons |
|--------|------|------|
| Single `journal_actions` with `outcome = 'log'` rows | Fewer tables, one chronological view of run output, simpler queries | Logs vs return values are different concepts; querying "what did strategy return" requires `WHERE outcome IN ('actions','noop','validation_error','runtime_error')` |
| Separate `journal_actions` + `journal_logs` | Clean separation; logs can have a different schema later (level, message_text) | Two tables for a small amount of data |

**Recommendation: separate `journal_logs`** (id, run_id, message, recorded_at) so the "what happened" vs "what did the agent decide" axes stay distinct. Adds one table but the cost is trivial and the read-side query stays clean. Open for discussion in `gsd-discuss-phase`.

### Schema migration approach

D-03b (Phase 2 CONTEXT) declared `CREATE TABLE IF NOT EXISTS` with no migration crate. Phase 3 follows that pattern: append the new tables to `SCHEMA_SQL` in `crates/executor-state/src/schema.rs`. Idempotent on re-open. **No `schema_version` table introduced yet** — defer until a destructive migration is needed.

### Repository layer additions

In `crates/executor-state/`:
- New module `journal.rs` (or extend `runs.rs`) with free functions `record_source_read`, `record_action_outcome`, `record_log`, `list_source_reads_for_run`, `list_actions_for_run`, `list_logs_for_run`.
- `StateStore` façade methods: `record_source_read`, `record_action_outcome`, `record_log` (mutating), and read-side accessors for tests.
- All gated under the same `Mutex<StateStore>` boundary — same `spawn_blocking` pattern.

### Run lifecycle

```text
strategy_run_once handler enters:
  1. validate input (strategy_id format)                              [no DB, no JS]
  2. spawn_blocking { state.lock(); store.get_strategy_by_id(id) }     [DB read]
     → if None or deleted_at != NULL: error -32014 (existing) / -3201X strategy_deleted
  3. spawn_blocking { state.lock(); store.insert_run(strategy_id, Queued); }
     ↳ run_id returned; store.update_run_status(run_id, Running)
  4. spawn_blocking { state.lock(); store.record_source_read(run_id, "strategy_source", strategy_id) }
  5. spawn_blocking { run JS in fresh Runtime+Context; collect return value or error }
     → memory/timeout/exception are RuntimeError
  6. spawn_blocking { state.lock(); validate return value }
     → noop | Action[] → record_action_outcome(actions/noop)
     → invalid shape → record_action_outcome(validation_error)
     → runtime error → record_action_outcome(runtime_error)
  7. spawn_blocking { state.lock(); store.update_run_status(run_id, Succeeded | Failed) }
  8. respond
```

Every database-touching block is its own `spawn_blocking` so the tokio mutex is **never held across an await** (Pitfall 4 from Phase 2 RESEARCH). The JS engine block is also `spawn_blocking` because rquickjs is sync and we don't want to occupy the main runtime thread.

[ASSUMED] That step 5's `spawn_blocking` and the DB `spawn_blocking`s can each acquire the mutex independently without deadlock — they don't run concurrently within a single `strategy_run_once` invocation. Verified by inspection of the linear flow above. **No nested locking.**

## Action[]/noop Output Schema

The strategy's return value is converted from `rquickjs::Value` to `serde_json::Value` and validated against the existing `executor-core::schema::action::Action` enum.

### Wire shape (Phase 3 valid forms)

```jsonc
// Form 1: literal noop
"noop"

// Form 2: empty action array (equivalent to noop in semantics, distinct in journal)
[]

// Form 3: array of Action variants
[
  { "kind": "noop" }
  // Phase 4 adds: contract_call, raw_call, erc20_approve, erc20_transfer, native_transfer
]
```

### Validation algorithm

```rust
fn validate_strategy_output(v: serde_json::Value) -> Result<ValidatedOutput, OutputError> {
    match v {
        serde_json::Value::String(s) if s == "noop" => Ok(ValidatedOutput::Noop),
        serde_json::Value::Array(items) => {
            let actions: Vec<Action> = items.into_iter()
                .enumerate()
                .map(|(i, item)| serde_json::from_value::<Action>(item)
                    .map_err(|e| OutputError::InvalidActionAt { index: i, detail: e.to_string() }))
                .collect::<Result<_, _>>()?;
            Ok(ValidatedOutput::Actions(actions))
        }
        other => Err(OutputError::WrongTopLevelShape {
            actual: type_name(&other).to_string(),  // "object" / "number" / "boolean" / "null"
        }),
    }
}
```

### Action enum extension

`crates/executor-core/src/schema/action.rs` currently has only `Noop`. **Phase 3 does not add new variants** — Phase 4 owns CTX-01..09. The existing `#[serde(tag = "kind", rename_all = "snake_case")]` already accepts `{"kind":"noop"}`. The validation code above is forward-compatible: when Phase 4 adds `ContractCall`, the same `serde_json::from_value::<Action>` call deserialises it.

### Error envelope shape (returned from MCP tool)

When validation or runtime fails, the tool returns an MCP error (not a success result with error fields), so the agent sees a structured failure. This matches Phase 2's `map_state_error` pattern.

```text
-3201X strategy_invalid_output  → JSON-RPC error response, data = { code, detail, run_id, journal_action_id }
-3201Y strategy_runtime_error   → same envelope, data.kind ∈ "exception"|"oom"|"timeout"|"stack_overflow"
-3201Z strategy_deleted         → strategy was soft-deleted; can't run
```

Even though the run row exists with status `Failed` and the journal records the failure, the **tool result is the error**. Agents that want to reconstruct partial state can call `execution_get { execution_id: run_id }` (already wired in Phase 2).

## MCP Error Code Plan

Phase 2 used `-32014` / `-32015` / `-32016` for storage errors and `-32602` for validation. Phase 3 adds three new codes in the JSON-RPC server-defined range (`-32000..-32099`).

| Code | Symbol (proposed) | When emitted | `data.code` string |
|------|-------------------|--------------|---------------------|
| `-32011` | `STRATEGY_DELETED` | `strategy_run_once` against a soft-deleted strategy (D-02c) | `"strategy_deleted"` |
| `-32017` | `STRATEGY_RUNTIME_ERROR` | JS exception, OOM, timeout, stack overflow | `"strategy_runtime_error"` (with `data.kind` for sub-classification) |
| `-32018` | `STRATEGY_INVALID_OUTPUT` | Return value isn't `"noop"` / `Action[]` | `"strategy_invalid_output"` |

[ASSUMED] `-32011` is unused. Phase 2 used 14/15/16, and the existing `unimplemented_err` uses `-32010`. **Plan must verify** by grepping `~/.cargo/registry/src/.../rmcp-1.5.0/` (Plan 02-02 already did this for -32014..16; same approach).

`-32012` and `-32013` reserved for future Phase 3 needs (e.g., a separate timeout code if we want timeout to be distinguishable from other runtime errors at the MCP level — currently `data.kind = "timeout"` covers it).

`-32019` reserved for Phase 4.

### Why three codes (not one giant "strategy_failed")

Agent UX: an agent looking at `data.code = "strategy_invalid_output"` knows to fix the strategy source. `data.code = "strategy_runtime_error"` with `kind = "timeout"` knows to optimise. `data.code = "strategy_deleted"` knows to re-register. Lumping them all into one code forces every agent to parse free-form messages.

### Helper module additions

```rust
// crates/executor-mcp/src/errors.rs (extend, do not rewrite)
pub const STRATEGY_DELETED: ErrorCode = ErrorCode(-32011);
pub const STRATEGY_RUNTIME_ERROR: ErrorCode = ErrorCode(-32017);
pub const STRATEGY_INVALID_OUTPUT: ErrorCode = ErrorCode(-32018);

pub fn strategy_runtime_error(kind: &'static str, message: String, run_id: &str) -> McpError { ... }
pub fn strategy_invalid_output(detail: String, run_id: &str) -> McpError { ... }
pub fn strategy_deleted(strategy_id: &str) -> McpError { ... }
```

## strategy_run Tool Wiring

### Input (existing — no change needed)

```rust
// executor-core::schema::strategy::StrategyRunOnceInput  (already defined Phase 1)
pub struct StrategyRunOnceInput {
    pub strategy_id: String,
}
```

D-09a regex `^[0-9a-f]{64}$` validation is reused via `validation::validate_strategy_id_format`.

### Output (new — add to executor-core)

```rust
// crates/executor-core/src/schema/execution.rs
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Response for strategy_run_once (Phase 3).")]
pub struct StrategyRunOnceResponse {
    pub run_id: String,
    pub strategy_id: String,
    pub status: RunStatus,           // 'succeeded' or 'failed' (Phase 3 never returns mid-flight)
    pub started_at: String,
    pub finished_at: String,         // always populated since Phase 3 runs to completion
    pub outcome: StrategyOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StrategyOutcome {
    Noop,
    Actions { actions: Vec<Action> },
    // validation_error / runtime_error are surfaced as MCP errors, not as response variants
}
```

A new schema golden `StrategyRunOnceResponse.json` joins the Phase 2 set.

### Handler skeleton

```rust
#[tool(name = "strategy_run_once", description = "Execute a registered JavaScript strategy once in a sandbox. Returns the validated Action[] or 'noop'. Runtime/validation errors become structured MCP errors with a run_id reference for journal lookup.")]
async fn strategy_run_once(
    &self,
    Parameters(input): Parameters<StrategyRunOnceInput>,
) -> Result<CallToolResult, McpError> {
    validate_strategy_id_format(&input.strategy_id).map_err(invalid_params)?;

    // Step 1: load strategy
    let state = self.state.clone();
    let id = input.strategy_id.clone();
    let strategy = tokio::task::spawn_blocking(move || {
        let store = state.blocking_lock();
        store.get_strategy_by_id(&id)
    }).await
        .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
        .map_err(map_state_error)?
        .ok_or_else(|| map_state_error(StateError::NotFound(format!("strategy {}", input.strategy_id))))?;

    if strategy.deleted_at.is_some() {
        return Err(strategy_deleted(&strategy.id));
    }

    // Step 2: insert run (Queued → Running)
    // Step 3: record source read
    // Step 4: spawn_blocking the JS execution
    // Step 5: validate output
    // Step 6: record action outcome
    // Step 7: update run status (Succeeded or Failed)
    // Step 8: return StrategyRunOnceResponse OR error
    ...
}
```

Each step with a comment is a separate `spawn_blocking` boundary in the implementation.

### Wiring impact summary

| File | Change |
|------|--------|
| `crates/strategy-js/` (NEW) | new crate; rquickjs dep; `Sandbox` struct + `run_once(source, ctx_input) -> Result<JsonValue, RunnerError>` |
| `crates/executor-state/src/schema.rs` | append `journal_source_reads` + `journal_actions` (+ optional `journal_logs`) to `SCHEMA_SQL` |
| `crates/executor-state/src/journal.rs` (NEW) or `runs.rs` extension | repository functions for new tables |
| `crates/executor-state/src/store.rs` | new façade methods `record_source_read`, `record_action_outcome`, `record_log` |
| `crates/executor-core/src/schema/execution.rs` | add `StrategyRunOnceResponse` + `StrategyOutcome` |
| `crates/executor-mcp/Cargo.toml` | add `strategy-js` workspace dep |
| `crates/executor-mcp/src/server.rs` | add `runner: Arc<strategy_js::Sandbox>` (or just construct per-call inside the handler — see open question) |
| `crates/executor-mcp/src/errors.rs` | add three new error codes + helpers |
| `crates/executor-mcp/src/tools.rs` | replace `strategy_run_once` placeholder with real handler; remove `Err(unimplemented_err(...))` |
| `crates/executor-mcp/tests/stdio_handshake.rs` | new integration tests (see Test Strategy) |
| `Cargo.toml` (workspace) | add `crates/strategy-js` to `members` |

## Concurrency Plan

**Phase 3 keeps the Phase 2 invariant: serialised access through `Arc<tokio::sync::Mutex<StateStore>>`.** No connection pool, no inner mutex.

JavaScript execution adds a second concern: rquickjs `Runtime` is `!Sync` by default (it's `Send + Sync` only with the `parallel` feature, which we are not enabling). [VERIFIED: docs.rs/rquickjs/latest/rquickjs/struct.Runtime.html threading characteristics]

**Pattern:**

- Per `strategy_run_once` invocation, create a **fresh** `Runtime` + `Context` inside `spawn_blocking`. This makes `Send`/`Sync` constraints irrelevant — the runtime never crosses thread boundaries because it's created and dropped on the same blocking-pool worker.
- The `tokio::sync::Mutex<StateStore>` is acquired only for DB calls, never held across the JS execution `spawn_blocking`. The JS execution `spawn_blocking` block does **not** acquire the storage mutex.
- Two concurrent `strategy_run_once` calls each get their own `Runtime`+`Context` and serialize on the storage mutex when they touch the DB. JS execution itself runs in parallel on the blocking pool. This is fine because JS execution is pure (no shared state outside the storage layer).

**Non-pattern (rejected):** Reusing a single long-lived `Runtime` across runs. QuickJS state can leak between runs (globals from script A visible to script B). Fresh runtime per run is cheap (`<10ms` to construct + base intrinsics) and gives clean isolation.

**No nested locking, no deadlock paths, no `await` while holding the mutex.** Same as Phase 2.

[ASSUMED] Construction cost of `Runtime + Context::base` is well under 50ms. **Plan must measure** — if it's slower than expected, we can pool `Runtime` instances behind a `Mutex<Vec<Runtime>>` and reset their context between uses. Defer until measured.

## Pitfalls & Gotchas

1. **`Context::full` includes intrinsics we don't want.** Use `Context::base` to start from the minimum; if a strategy genuinely needs `Map`, `Set`, `Promise`, `JSON` we add them via `Context::builder` selectively. **Test:** `typeof JSON !== 'undefined'` (we DO want JSON). Verify what `base` actually includes — [ASSUMED] it includes `JSON`, `Math`, basic types.

2. **Interrupt handler may not fire during pure-Rust callbacks.** The interrupt is checked between bytecode instructions. If a strategy calls a host function (e.g., `ctx.log`) and that host function blocks (it shouldn't, but still), the interrupt won't fire until control returns to the interpreter. **Mitigation:** keep all host functions trivially fast (no DB writes inside `ctx.log`'s synchronous call — buffer in a `Vec<String>` and drain after eval returns; the journal write happens on the main thread post-eval).

3. **`set_memory_limit(0)` means UNLIMITED, not zero.** [VERIFIED: docs.rs/rquickjs/latest/rquickjs/struct.Runtime.html#method.set_memory_limit] Easy bug if a config field defaults to `0` for "use default". Use a sentinel like `Option<usize>` and unwrap to a hardcoded constant.

4. **`set_memory_limit` is a no-op with custom allocators.** [VERIFIED: docs.rs/rquickjs/latest/rquickjs/struct.Runtime.html#method.set_memory_limit] Don't combine with mimalloc/jemalloc swapped at the rquickjs level. We use the default allocator → fine.

5. **rquickjs Value lifetimes are 'js-bound to Context::with.** You cannot return an `rquickjs::Value` past the `with` closure. Convert to `serde_json::Value` inside the closure. There's a `rquickjs::Value::into_string()` and friends for primitives; arrays/objects need iteration.

6. **`!Sync` Runtime + tokio.** Don't store `Runtime` in `Arc<Mutex<>>` and use it from async. Always construct it inside `spawn_blocking`. The orchestrator-prescribed pattern (fresh per run inside `spawn_blocking`) sidesteps this entirely.

7. **Strategy sources can throw inside top-level code, not just inside the entry function.** If the source is `throw new Error("nope")`, that throws during `eval`. Wrap the `eval` call's error and journal it as `runtime_error`.

8. **JSON.stringify on QuickJS values that contain BigInt.** QuickJS supports BigInt; `JSON.stringify(1n)` throws. Strategies should not include BigInts in their `Action[]` return value (Phase 4's helpers will convert; Phase 3 has no use case). Document and test: a strategy returning `[1n]` produces a `validation_error`.

9. **Goldens for new schemas.** Phase 2 introduced 14 schema goldens; Phase 3 adds at minimum `StrategyRunOnceResponse.json` and updates none of the existing ones. Run `UPDATE_SCHEMAS=1 cargo test -p executor-core --test schema_snapshots`.

10. **WAL artifacts in tests.** Phase 2 added `state.db-shm` / `state.db-wal` to `.gitignore`. Phase 3 doesn't change this; just inherit. New tables don't change the WAL footprint.

11. **`sha2` reuse vs recompute.** `strategy.id` is already `hash_source(strategy.source)`. Don't recompute when journaling the source read — pass the existing `id` as the `target` field.

12. **Race between strategy soft-delete and `strategy_run_once`.** v1 single-writer Mutex closes this: load+delete-check happens under the same mutex acquisition window as the run insert. T-02-01-06-style accepted residual race not present in Phase 3 either.

13. **No `print` / `console` means strategies can't debug themselves without `ctx.log`.** Document this in the prompt for `prompt_strategy_authoring` (Phase 7 prompt content). Phase 3 adds `ctx.log` to the surface so the deficit is small.

14. **rquickjs error type variance.** `rquickjs::Error::Exception` carries the JS exception value, not a string. Convert via `ctx.exception().and_then(|e| e.into_string())` or similar. Pattern-match exhaustively so `Error::OutOfMemory`, `Error::Panic`, `Error::IntoJs`, etc. all map to the right `kind` in `strategy_runtime_error`.

## Open Questions (RESOLVED in 03-CONTEXT.md)

> **All questions in this section were resolved during planner-locked context capture (`/gsd-discuss-phase` was skipped per orchestrator brief).** This section is retained for historical traceability — every question below is answered by a locked `D-XX` decision in `03-CONTEXT.md`. Read CONTEXT for the binding answer; this section just records the original framing.
>
> Resolution map:
> - Q1 (entry-point shape) → **D-05** (Shape B locked)
> - Q2 (separate `journal_logs` table) → **D-06** (three separate tables)
> - Q3 (wall-clock default) → **D-03** (2 s)
> - Q4 (memory cap default) → **D-03** (64 MiB)
> - Q5 (`strategy-js` as new crate) → **D-02** (new `crates/strategy-js/`)
> - Q6 (`Runtime` reuse vs fresh per run) → **deferred to Phase 6 pool** (fresh-per-run for v1; pool only if Wave-0 measurement > 50 ms)
> - Q7 (`ctx.log` immediate vs buffered) → **D-04** (buffered logs)
> - Q8 (MCP error code numbers) → **D-07** (-32011 / -32017 / -32018)
> - Q9 (Promise return handling) → **D-10** (rejected as `STRATEGY_INVALID_OUTPUT`)
> - Q10 (`Action::Noop` survival) → Action wire-shape stays canonical (Phase 4 extends the variant set; `Noop` remains valid)

1. **Strategy entry-point shape: A or B?** (See *Strategy entry-point shape* above.) Recommendation: **B** (`(ctx) => ...` arrow). Cleaner contract; rejects ambiguous styles. Force the agent to write `(ctx) => "noop"` or `(ctx) => [{kind:"noop"}]`. This is a stronger contract than letting top-level expressions or `globalThis.strategy` work. → Decide in `gsd-discuss-phase`.

2. **Separate `journal_logs` table or fold into `journal_actions`?** Recommendation: separate table. Keeps "what did the strategy decide" cleanly separable from "what did the strategy say."

3. **Wall-clock default: 2 seconds or different?** Recommendation: 2s for v1. The runtime loop is "agent calls tool, tool runs, returns" — 2s of pure JS computation is *generous* for v1 strategies which are all small EVM read+decision logic. Later config-tunable.

4. **Memory cap default: 64 MiB or smaller?** Recommendation: 64 MiB. QuickJS itself uses ~few MiB; 64 MiB leaves room for strategies that build up large in-memory state (e.g., parse a long ABI). Smaller (16 MiB) might surprise legitimate uses.

5. **`strategy-js` as a new crate, or staged inside `executor-mcp`?** AGENTS.md target architecture explicitly lists `strategy-js/` as a workspace member. Recommendation: **create the crate now** in Phase 3. It costs almost nothing and matches the documented target. Phase 4 will extend it (the crate's `Sandbox` struct grows `ctx.evm.*` injection in Phase 4).

6. **Should `Runtime` be reused across runs (pool of size 1) or freshly constructed?** Recommendation: fresh per run for v1. Measure construction cost in Wave 0; pool only if it's > 50ms.

7. **`ctx.log` immediate-flush vs buffered-flush?** Recommendation: buffer in a `Vec<String>` during JS execution (the host function appends; doesn't write DB), then flush all rows in one transaction after `eval` returns. Simpler concurrency story; one DB lock acquisition per run instead of N.

8. **MCP error codes -32011/-32017/-32018 — which numbers?** Recommendation as proposed. Verify nothing in `rmcp 1.5` claims them (Plan 02-02 already did this for -32014..16; same grep).

9. **Handle a strategy that returns a Promise.** With no `futures` feature and `Context::base`, the strategy *can* still create a Promise (the Promise constructor is an intrinsic). What if the strategy returns one? Recommendation: validation rejects it (Promise is not `"noop"` and not a plain array). `data.detail = "promise return not supported in v1; strategies must be synchronous"`. Test included.

10. **Does the schema `Action::Noop` variant survive Phase 4?** Yes. Phase 4 will add ContractCall etc., but Noop remains a valid (no-op) variant. Existing schema golden at `crates/executor-core/tests/schemas/Action.json` (if generated) needs to be checked.

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | `cargo test` with `#[tokio::test]` for stdio integration; `#[test]` for repository unit tests |
| Config file | None — Phase 2 pattern continued |
| Quick run command | `cargo test -p strategy-js --lib && cargo test -p executor-state --test journal_roundtrip && cargo test -p executor-mcp --test stdio_handshake -- strategy_run_once_` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| STR-03 | A registered strategy returning `"noop"` runs and produces a `Succeeded` run | integration (stdio) | `cargo test -p executor-mcp --test stdio_handshake strategy_run_once_returns_noop` | ❌ Wave 0 (test added in Phase 3) |
| STR-04 | `typeof require/process/fs/fetch/setTimeout === 'undefined'` inside the sandbox | integration | `cargo test -p strategy-js --lib sandbox_blocks_host_globals` | ❌ Wave 0 |
| STR-04 | Wall-clock interrupt fires on `while(1){}` | unit | `cargo test -p strategy-js --lib wall_clock_interrupt_terminates_infinite_loop` | ❌ Wave 0 |
| STR-04 | Memory limit fires on `let a=[]; while(1)a.push(new Array(1e6))` | unit | `cargo test -p strategy-js --lib memory_limit_terminates_oom_strategy` | ❌ Wave 0 |
| STR-05 | Returning `42` produces `STRATEGY_INVALID_OUTPUT` | integration | `cargo test -p executor-mcp --test stdio_handshake strategy_run_once_rejects_number_return` | ❌ Wave 0 |
| STR-05 | Returning `[{kind:"noop"}]` succeeds | integration | `cargo test -p executor-mcp --test stdio_handshake strategy_run_once_accepts_action_array` | ❌ Wave 0 |
| STR-05 | Returning `[{kind:"contract_call",...}]` fails (Phase 3 — Action only has Noop variant) | integration | `cargo test -p executor-mcp --test stdio_handshake strategy_run_once_rejects_phase4_action_kind` | ❌ Wave 0 |
| STR-05 | Returning a Promise fails | integration | `cargo test -p executor-mcp --test stdio_handshake strategy_run_once_rejects_promise_return` | ❌ Wave 0 |
| STJ-03 | After a successful run, `journal_source_reads` has exactly one row with `kind='strategy_source'` | integration | `cargo test -p executor-state --test journal_roundtrip source_read_recorded_per_run` | ❌ Wave 0 |
| STJ-04 | After a noop run, `journal_actions` has one row with `outcome='noop'` | integration | `cargo test -p executor-state --test journal_roundtrip noop_outcome_recorded` | ❌ Wave 0 |
| STJ-04 | After a validation-error run, `journal_actions` has one row with `outcome='validation_error'` | integration | `cargo test -p executor-state --test journal_roundtrip validation_error_recorded` | ❌ Wave 0 |
| STJ-04 | After a runtime-error run, `journal_actions` has one row with `outcome='runtime_error'` and `payload_json.kind='timeout'` | integration | `cargo test -p executor-state --test journal_roundtrip timeout_recorded_with_kind` | ❌ Wave 0 |

### Sampling Rate

- **Per task commit:** quick command above (~5–10s).
- **Per wave merge:** `cargo test --workspace`.
- **Phase gate:** Full suite green + clippy clean before `/gsd-verify-work`.

### Wave 0 Gaps

- [ ] `crates/strategy-js/` — new crate; needs `Cargo.toml`, `src/lib.rs`, `src/sandbox.rs`, plus unit tests inline.
- [ ] `crates/executor-state/tests/journal_roundtrip.rs` — new integration test file for new tables.
- [ ] `crates/executor-core/tests/schemas/StrategyRunOnceResponse.json` — new golden (regenerate via `UPDATE_SCHEMAS=1`).
- [ ] Test fixtures: deterministic strategy sources for each test case. Recommendation: inline in test files as `const NOOP_STRATEGY: &str = "(ctx) => \"noop\"";` etc. — no separate fixture directory needed for v1.
- [ ] Add `strategy-js` to workspace `members` in root `Cargo.toml`.
- [ ] Phase 1 `unimplemented_tools_return_phase_hint` test must drop `strategy_run_once` from its case list (now implemented).

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | Phase 3 has no auth surface — local stdio MCP, single operator. |
| V3 Session Management | no | Same. |
| V4 Access Control | yes | Sandbox confinement is access control: strategy code is the untrusted principal, host is the trusted principal. Default-deny via `Context::base` + zero exposed host APIs. |
| V5 Input Validation | yes | Strategy source size cap (Phase 2 D-09 already enforces 256 KiB); return-shape validation (this phase). |
| V6 Cryptography | no | No new crypto surface in Phase 3. |

### Known Threat Patterns for rquickjs sandbox

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Strategy uses `eval`/`Function` to compile arbitrary JS | E (Elevation of Privilege) | Acceptable: compiled JS runs under same intrinsics + same limits. Not a sandbox escape. |
| Strategy busy-loops to deny service | D (DoS) | `set_interrupt_handler` with wall-clock check; uncatchable exception terminates. |
| Strategy allocates to OOM the host process | D | `set_memory_limit` with sentinel before unlimited; QuickJS raises OOM exception. |
| Strategy recurses to stack-overflow the host | D | `set_max_stack_size` (default 256 KiB; we set 1 MiB); QuickJS raises StackOverflow. |
| Strategy attempts to load native modules | E | `dyn-load` feature **not enabled**; `import("./foo.so")` has no resolver. |
| Strategy attempts dynamic `import()` of remote URLs | I (Information Disclosure) / E | `loader` feature **not enabled**; no module resolver registered. |
| Strategy reads strategy source of other strategies | I | `ctx` does not expose `StateStore`. Phase 4 must keep the same posture for `ctx.evm.*` (no read of arbitrary host paths). |
| Strategy starts a process / spawns thread | E | `process`, `Worker`, `child_process` not exposed. |
| Strategy reads filesystem | I | `fs`, `node:fs`, `Deno.readFile` etc. not exposed. |
| Strategy makes network calls | I | `fetch`, `XMLHttpRequest`, `WebSocket` not exposed. Phase 4's `ctx.evm.*` must use the **runtime's** RPC client, never expose RPC primitives to JS. |
| Side-channel via wall-clock timing | I | Out of scope for v1; document as accepted. |
| Side-channel via memory-pressure observation (allocation latency) | I | Out of scope for v1. |

[VERIFIED: rquickjs README] explicit statement that the crate "doesn't aim to provide system and web APIs" — none of the network/fs/process APIs exist by default.

[VERIFIED: docs.rs/rquickjs/latest/rquickjs/index.html] documented features — `loader`, `dyn-load` are opt-in.

## Project Constraints (from CLAUDE.md and AGENTS.md)

From `./AGENTS.md`:
- **`rquickjs` is the chosen sandbox** — non-negotiable [AGENTS.md line 22]. Confirmed compatible with research.
- **Stdout discipline** — workspace `clippy::print_stdout = "deny"` [Cargo.toml line 28]. New `strategy-js` crate inherits via workspace lints.
- **Strategy code must not access** filesystem/network/process/private keys [AGENTS.md line 60]. Phase 3 enforces by NOT injecting these into `ctx`.
- **Strategy returns `Action[]`; does not sign or broadcast** [AGENTS.md line 61]. Validation rejects anything else.
- **Local signer behind boundary** [AGENTS.md line 63] — Phase 3 has no signer at all; this constraint binds Phase 6.

From `.planning/PROJECT.md` Constraints §:
- **Runtime boundary**: enforced by sandbox isolation contract above.
- **Observability**: every run journals — STJ-03 + STJ-04 cover this for Phase 3.
- **Scope control**: no dashboard, marketplace, scheduler — Phase 3 introduces none.

There is no `./CLAUDE.md` at the project root (only the user's global `~/.claude/CLAUDE.md`, which is project-agnostic).

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | rquickjs default features do not pull in `loader`/`dyn-load`/`futures`/`parallel` | JS Sandbox Recommendation, Cargo dependency | Plan must verify with `cargo tree`. If wrong, set `default-features = false` and explicitly enable only what's needed. Sandbox compromise if `loader` accidentally enabled. |
| A2 | `Context::base` excludes `module`/`import`/`require` intrinsics | ctx Host API caveat | Unlikely (these aren't traditional ECMA intrinsics) but plan must include test asserting `typeof require === 'undefined'` and dynamic import returns rejected promise / throws. |
| A3 | Construction cost of `Runtime + Context::base` is < 50ms | Concurrency Plan | If slower, two-strategy throughput is reduced. Mitigation: pool runtimes. Measure in Wave 0. |
| A4 | `-32011`, `-32017`, `-32018` are unused by rmcp 1.5 internals | MCP Error Code Plan | If used, collision causes ambiguous error responses. Plan grep step (same approach as Plan 02-02) catches this. |
| A5 | No nested locking risk in proposed run lifecycle | Source-Read & Journal Capture Strategy | Verified by inspection of linear flow. If implementation detail introduces nesting, deadlock possible. Test: parallel `strategy_run_once` calls don't deadlock under contention. |
| A6 | `chrono::Utc::now()` is suitable for `ctx.now()` injection | ctx Host API | If determinism is needed for testing, inject a clock. v1 uses `chrono` directly; tests can wrap with a feature flag if needed. |
| A7 | The 7 RunStatus variants suffice for Phase 3 outcomes (we use `Succeeded` and `Failed` only) | Source-Read & Journal Capture Strategy | Confirmed: validation errors and runtime errors map to `Failed`. The structured detail goes in `journal_actions.payload_json`, not the run row. |

## Code Examples

### Verified pattern: rquickjs sandbox with all three limits

```rust
// Source: docs.rs/rquickjs/latest/rquickjs/struct.Runtime.html (verified 2026-04-27)
use rquickjs::{Runtime, Context};
use std::time::{Duration, Instant};

fn run_strategy(source: &str, wall_clock_ms: u64) -> Result<serde_json::Value, RunnerError> {
    let rt = Runtime::new().map_err(RunnerError::engine)?;
    rt.set_memory_limit(64 * 1024 * 1024);
    rt.set_max_stack_size(1024 * 1024);
    rt.set_gc_threshold(8 * 1024 * 1024);

    let deadline = Instant::now() + Duration::from_millis(wall_clock_ms);
    rt.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));

    let ctx = Context::base(&rt).map_err(RunnerError::engine)?;
    ctx.with(|c| -> Result<serde_json::Value, RunnerError> {
        // Inject ctx (see ctx Host API section)
        // ... ctx.set("ctx", make_ctx_object(c)?)?;
        let value: rquickjs::Value = c.eval(source)
            .map_err(|e| classify_qjs_error(e))?;
        // Convert rquickjs::Value to serde_json::Value INSIDE this closure.
        json_from_qjs(value).map_err(RunnerError::conversion)
    })
}
```

### Verified pattern: Phase 2 spawn_blocking storage call (reuse for Phase 3 DB calls)

```rust
// Source: crates/executor-mcp/src/tools.rs lines 60-72 (in-tree, Phase 2)
let state = self.state.clone();
let outcome = tokio::task::spawn_blocking(move || {
    let mut store = state.blocking_lock();
    store.register_strategy(...)
})
.await
.map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
.map_err(map_state_error)?;
```

Phase 3 reuses this exact shape for `get_strategy_by_id`, `insert_run`, `update_run_status`, `record_source_read`, `record_action_outcome`.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Hand-rolled JS sandbox via process isolation | In-process engine with default-deny intrinsics | rquickjs / QuickJS-NG era (2023+) | Faster (no fork), simpler (no IPC) — but only safe if engine has hard resource limits, which rquickjs does |
| `quickjs` crate (Bellard original, abandoned bindings) | rquickjs (active fork) | 2022+ | rquickjs has the active maintainer ecosystem; AWS LLRT depends on it |
| Boa as a sandbox | Boa as a JS engine (sandbox not yet a goal) | persistent | Boa is faster/safer-Rust but unsafe to embed for untrusted code today |

**Deprecated/outdated:**
- Running untrusted JS via `node:vm` from Rust via subprocess — fragile, slow, dependency on Node binary.
- `js-sandbox` crate (uses Deno) — heavyweight; abandoned for non-Deno-native use cases.

## Sources

### Primary (HIGH confidence)
- `docs.rs/rquickjs/latest/rquickjs/struct.Runtime.html` — `set_memory_limit`, `set_max_stack_size`, `set_interrupt_handler`, `set_gc_threshold` all documented. Threading note: `Send+Sync` only with `parallel` feature.
- `docs.rs/rquickjs/latest/rquickjs/struct.Context.html` — `Context::base`, `Context::full`, `Context::custom`, `Context::builder`.
- `docs.rs/rquickjs/latest/rquickjs/index.html` — feature flags (`futures`, `loader`, `dyn-load`, `macro`, `parallel`, `either`, `indexmap`).
- `crates.io/api/v1/crates/rquickjs` — version 0.11.0, published 2025-12-24, 1.69M downloads (verified 2026-04-27 via curl).
- `github.com/DelSkayn/rquickjs` README — explicit statement that "rquickjs doesn't aim to provide system and web APIs"; AWS LLRT use confirmed.
- `crates/executor-mcp/src/tools.rs` (in-tree) — Phase 2 `spawn_blocking` pattern proven, lines 60-72.
- `crates/executor-mcp/src/errors.rs` (in-tree) — Phase 2 error envelope established for extension.
- `crates/executor-state/src/schema.rs` (in-tree) — `SCHEMA_SQL` extension point.
- `crates/executor-core/src/schema/action.rs` (in-tree) — `Action::Noop` variant already in place.

### Secondary (MEDIUM confidence)
- WebSearch result confirming `Context::full` registers all standard intrinsics; `Context::custom` enables selective inclusion.
- `boajs.dev` + Hacker News / x-cmd release notes confirming Boa lacks sandbox model.
- `crates.io` API for `boa_engine`, `deno_core`, `v8`, `quickjs-rusty` version verification.

### Tertiary (LOW confidence — flagged for Wave 0 verification)
- A1: rquickjs default features list. Plan must run `cargo tree -p strategy-js` and assert no unwanted transitive deps.
- A2: `Context::base` exclusion of module/import/require — needs runtime test.
- A4: rmcp 1.5 not using `-32011`/`-32017`/`-32018` — needs grep of cargo registry.

## Metadata

**Confidence breakdown:**
- JS engine choice (rquickjs): **HIGH** — verified version, verified resource-limit APIs, verified default sandbox posture, verified license.
- Resource limits API: **HIGH** — three independent docs.rs methods directly applicable.
- ctx host API surface: **HIGH** — keep minimal per phase requirements.
- Journal table design: **MEDIUM** — choice between single-table-with-outcome vs separate-logs-table is a discuss-phase question.
- Concurrency plan: **HIGH** — direct extension of Phase 2 verified pattern.
- MCP error codes: **MEDIUM** — code numbers are proposals; needs grep verification.
- Pitfalls: **MEDIUM** — Pitfalls 1, 2, 4, 5 confirmed by docs; Pitfall 3 ("`set_memory_limit(0)` is unlimited") verified directly.

**Research date:** 2026-04-27
**Valid until:** ~30 days (rquickjs is stable; QuickJS-NG underpinnings are stable; rmcp at 1.5 is stable).
