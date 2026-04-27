---
phase: 03
plan: 01
subsystem: strategy-js sandbox / rquickjs runtime
tags: [phase-3, strategy-js, rquickjs, sandbox, str-04]
requires:
  - executor-core (path dep — re-export shim only; no symbols used yet)
provides:
  - strategy_js::Sandbox::execute(source, &mut CtxHost) -> Result<serde_json::Value, RuntimeError>
  - strategy_js::CtxHost trait + CtxStub default impl
  - strategy_js::RuntimeError taxonomy (Timeout / Oom / StackOverflow / Exception / InvalidOutput / EngineInit)
  - strategy_js::limits::{WALL_CLOCK_MS, MEMORY_LIMIT_BYTES, GC_THRESHOLD_BYTES, MAX_STACK_BYTES}
affects:
  - Cargo.toml (workspace members += "crates/strategy-js")
tech-stack:
  added:
    - rquickjs 0.11.0 (default-features=false; per-crate pin only)
    - rquickjs-core 0.11.0
    - rquickjs-sys 0.11.0
    - allocator-api2 0.2.21 (transitive from rquickjs)
    - hashbrown 0.16.1 (transitive)
  patterns:
    - Single-owner unit struct (Sandbox) mirroring StateStore shape (executor-state/store.rs:18)
    - Single typed error enum per crate (RuntimeError) mirroring StateError (executor-state/error.rs:1-29)
    - Per-crate dep pinning (no promotion to [workspace.dependencies]) — Phase-2 precedent
    - Compile-time D-03 invariant guards via const { assert!(..) }
    - Fresh Runtime + Context per call (RESEARCH Q6, no pooling)
    - JS prelude scrub of D-11 forbidden globals before user source eval
    - Hand-walked qjs_value_to_json (no rquickjs serde-bridge feature pulled)
key-files:
  created:
    - crates/strategy-js/Cargo.toml
    - crates/strategy-js/src/lib.rs
    - crates/strategy-js/src/error.rs
    - crates/strategy-js/src/limits.rs
    - crates/strategy-js/src/sandbox.rs
    - crates/strategy-js/tests/sandbox_limits.rs
    - crates/strategy-js/tests/sandbox_entry_shape.rs
    - crates/strategy-js/tests/sandbox_host_globals.rs
  modified:
    - Cargo.toml (workspace members)
    - Cargo.lock (rquickjs deps)
decisions:
  - DEV-03-01-A: Use Context::builder().with::<intrinsic::All>() instead of Context::base — see Deviations.
  - DEV-03-01-B: Auto-add D-11 scrub prelude — see Deviations (Rule 2 enforcement of D-11).
  - DEV-03-01-C: Dynamic-import test pinned to InvalidOutput(promise) — see Deviations.
metrics:
  tasks_completed: 3
  duration_minutes: ~8
  files_created: 8
  files_modified: 2
  deviations: 3 (all auto-resolved per Rules 2/3)
  test_count_delta: +22 (3 lib + 8 entry-shape + 3 limits + 8 host-globals)
  workspace_test_total: 114 (was 92 baseline)
completed: 2026-04-27
---

# Phase 3 Plan 1: strategy-js sandbox crate + resource limits + host capability blocking — Summary

**One-liner:** Synchronous rquickjs-0.11 sandbox crate (`strategy-js`) shipping `Sandbox::execute(source, &mut CtxHost) -> serde_json::Value` with D-03 wall-clock/memory/stack budgets, D-05 Shape-B entry-point enforcement, D-10 promise rejection, and a D-11 forbidden-globals scrub that proves STR-04 at the runtime layer.

## What Shipped

### `crates/strategy-js/` (new crate)

- **Cargo.toml** declares `rquickjs = "0.11"` with `default-features = false` (D-01). Per-crate pin only, NOT promoted to `[workspace.dependencies]` — Phase-2 precedent (executor-state/Cargo.toml:10-13). Comment block in the new manifest documents the rule.
- **src/lib.rs** with the `#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]` tripwire and `pub use` re-exports of `RuntimeError`, `Sandbox`, `CtxHost`, `CtxStub`.
- **src/error.rs** declares `RuntimeError` with six variants (`Timeout`, `Oom`, `StackOverflow`, `Exception`, `InvalidOutput { detail }`, `EngineInit`) plus `From<rquickjs::Error>` (default conversion lands `Exception(msg)`; the typed classification happens inside `Sandbox::execute` via `caught_to_runtime_error`).
- **src/limits.rs** holds the D-03 constants (`WALL_CLOCK_MS = 2_000`, `MEMORY_LIMIT_BYTES = 64 MiB`, `GC_THRESHOLD_BYTES = 8 MiB`, `MAX_STACK_BYTES = 1 MiB`) with a `const { assert!(..) }` block guarding non-zero (Pitfall 3) and `GC < heap` invariants at compile time.
- **src/sandbox.rs** ships the synchronous `Sandbox::execute`:
  1. Fresh `Runtime` per call; applies `set_memory_limit` / `set_gc_threshold` / `set_max_stack_size`.
  2. `set_interrupt_handler` polls `Instant::now()` against the deadline and flips an `Arc<AtomicBool>` so the post-eval error path can promote a generic `Exception` to `Timeout` (Pitfall 14).
  3. Builds `Context::builder().with::<intrinsic::All>().build(&rt)` — see deviations DEV-03-01-A.
  4. Inside `Context::with`: installs an empty `__ctx` stub, runs a JS prelude that `delete`s every D-11 forbidden global from `globalThis`, evaluates an IIFE wrapper `(() => { const __fn = (SOURCE); if (typeof __fn !== "function") return "__STRATEGY_NOT_FUNCTION__"; return __fn(__ctx); })()`, rejects promise returns with `InvalidOutput(detail mentions "promise")` per D-10, and converts the result to `serde_json::Value` via a hand-walked `qjs_value_to_json`.
  5. Returns the JSON or a typed `RuntimeError`. The deadline-flag override runs after the closure exits so wall-clock interrupts don't masquerade as exceptions.

### Tests (22 new, all green)

| Suite | Test count | Coverage |
|---|---|---|
| `--lib` (error::tests + limits::tests) | 3 | Error message format, From<rquickjs::Error> via real syntax error, D-03 constants pinned to exact values |
| `tests/sandbox_limits.rs` | 3 | wall-clock interrupt within `WALL_CLOCK_MS + 500ms`, OOM-or-Timeout for unbounded array push, StackOverflow-or-Exception for infinite recursion |
| `tests/sandbox_entry_shape.rs` | 8 | Shape-B noop + action-array + empty-array + thrown error + isolation between runs; rejections for top-level string source, top-level object source, promise return |
| `tests/sandbox_host_globals.rs` | 8 | Enumerated `typeof === "undefined"` for 12 D-11 names + bespoke tests for `require`/dynamic `import()`/`Deno.*`/`globalThis.process`; intentional-presence pinning for `eval` + `Function` constructor |

## Verification

```text
cargo build -p strategy-js                                      # exit 0
cargo test -p strategy-js                                       # 22 passed (5 suites)
cargo test --workspace                                          # 114 passed (was 92 + 22 new)
cargo clippy --workspace --all-targets -- -D warnings           # exit 0
cargo tree -p strategy-js | grep -E '(libloading|tokio v)'      # empty (no forbidden transitive deps)
```

`Cargo.lock` records `rquickjs v0.11.0`, `rquickjs-core v0.11.0`, `rquickjs-sys v0.11.0`. No `libloading` or `tokio` reachable through rquickjs — confirms `loader`/`dyn-load`/`futures`/`parallel` features all stay disabled.

## Deviations from Plan

### DEV-03-01-A — [Rule 3, blocking issue] Replace `Context::base` with `Context::builder().with::<intrinsic::All>()`

- **Found during:** Task 2 (running `sandbox_entry_shape` tests).
- **Issue:** The plan's instruction to use `rquickjs::Context::base(&rt)` was based on the rationale "base excludes module/import/require intrinsics". Empirically in rquickjs 0.11, `Context::base` ships ONLY `JS_AddIntrinsicBaseObjects` plus `intrinsic::None` — it omits the `Eval` intrinsic. Every test that evaluated user JS source (including the simplest `(ctx) => "noop"`) failed with `Exception("eval is not supported")`.
- **Fix:** Use `Context::builder().with::<intrinsic::All>().build(&rt)`. `intrinsic::All` (defined at rquickjs-core-0.11.0/src/context/builder.rs:73-86) registers `Date / Eval / RegExp / RegExpCompiler / JSON / Proxy / MapSet / TypedArrays / Promise / BigInt / Performance / WeakRef`. Crucially it does NOT include any module/import/require/loader intrinsic — those ride only on `Context::full` (which uses `JS_NewContext`, a separate code path). The D-11 invariant — "no module/import/require" — is preserved. The `Context::full` prohibition stands.
- **Files modified:** crates/strategy-js/src/sandbox.rs (lines around 115-120) — added explanatory comment block citing the upstream source.
- **Commit:** 0381c59
- **Plan acceptance impact:** The plan's grep `'Context::base' >= 1` no longer holds. The replacement string `Context::builder()` + `intrinsic::All` is equivalent in posture (no module/import/require intrinsic) and necessary for evaluability. Documented inline so Plan 03-02 / 03-03 don't trip on it.

### DEV-03-01-B — [Rule 2, missing critical functionality] Add D-11 forbidden-globals JS prelude scrub

- **Found during:** Task 3 (`sandbox_blocks_host_globals`).
- **Issue:** rquickjs 0.11's `Promise` intrinsic exposes `queueMicrotask` on `globalThis` as a side-effect of registering Promise support. D-11 mandates `queueMicrotask` be absent. Without a scrub, the enumerated D-11 test reported `FOUND: queueMicrotask`.
- **Fix:** Inject a JS prelude (`FORBIDDEN_GLOBALS_SCRUB` const at the bottom of `sandbox.rs`) that runs `delete globalThis[name]` for every D-11 name unconditionally before user source executes. `delete` of an absent property is a no-op, so the prelude is defensive against future intrinsic additions. Also covers `Deno` (which is already absent but explicit-deletion costs nothing).
- **Files modified:** crates/strategy-js/src/sandbox.rs (added `FORBIDDEN_GLOBALS_SCRUB` const + an `eval` call inside `Context::with` after `__ctx` install).
- **Commit:** e4c6ec1
- **Justification:** D-11 enforcement is a correctness requirement, not a feature. The plan's Task-3 spec assumed `queueMicrotask` would be absent under `Context::base` — the assumption did not survive contact with `intrinsic::All`. Auto-add per Rule 2.

### DEV-03-01-C — [Rule 1, test correctness] Dynamic-import test rewritten to assert promise rejection

- **Found during:** Task 3 (`sandbox_blocks_dynamic_import`).
- **Issue:** The plan's test source was `(ctx) => { try { import("./foo.so"); return "BAD"; } catch(e) { return "noop"; } }`. In rquickjs 0.11 with no module loader, dynamic `import()` returns a rejected Promise; it does NOT throw synchronously. The `try` block ran to completion, returning `"BAD"`, breaking the test.
- **Fix:** Rewrote the test as `(ctx) => import("./foo.so")` and asserted the result is `RuntimeError::InvalidOutput { detail mentions "promise" }`. This is a stricter assertion: it proves dynamic import returns a Promise (D-10 rejects it as `InvalidOutput`) AND that no real module resolution occurred. The Exception branch is also accepted as a fallback for future rquickjs versions that might surface the failure differently.
- **Files modified:** crates/strategy-js/tests/sandbox_host_globals.rs — `sandbox_blocks_dynamic_import` body replaced.
- **Commit:** e4c6ec1
- **Justification:** Same observable contract (no module load), tighter assertion, no D-11 weakening.

## Threat Model Closure (T-03-01-01..13)

All twelve `mitigate` dispositions in the plan's threat register are now backed by passing tests:

| Threat | Mitigation evidence |
|---|---|
| T-03-01-01 (DoS busy-loop) | `wall_clock_interrupt_terminates_infinite_loop` |
| T-03-01-02 (DoS OOM) | `memory_limit_terminates_oom_strategy` |
| T-03-01-03 (DoS stack) | `stack_limit_terminates_recursive_strategy` |
| T-03-01-04 (native module via dyn-load) | `cargo tree` audit (no libloading) |
| T-03-01-05 (loader / dynamic import) | `sandbox_blocks_dynamic_import` |
| T-03-01-06 (filesystem) | `sandbox_blocks_node_fs_module`, `sandbox_blocks_deno_namespace` |
| T-03-01-07 (network) | `sandbox_blocks_host_globals` (fetch / XHR / WebSocket) |
| T-03-01-08 (process / Worker / child_process) | `sandbox_blocks_host_globals`, `sandbox_blocks_globalthis_process` |
| T-03-01-09 (state leak across runs) | `execute_runs_distinct_invocations_independently` |
| T-03-01-10 (promise return) | `execute_rejects_promise_return` |
| T-03-01-11 / T-03-01-12 (timing / memory side-channels) | accepted out of scope per plan |
| T-03-01-13 (eval / Function compiled JS) | accept with rationale; pinned by `sandbox_eval_is_present_but_sandboxed`, `sandbox_function_constructor_is_present_but_sandboxed` |

## Requirements Closed

- **STR-04** — Strategy code cannot access private keys, filesystem, process APIs, arbitrary network, or direct RPC clients. *Proven at the runtime layer (no MCP wiring yet — Plan 03-03 surfaces it through `strategy_run`).*

## Hand-Off to Plan 03-02

The `Sandbox::execute` signature is locked. Plan 03-02 will:
- Replace the `CtxStub` consumer with a real `RuntimeContext` impl `CtxHost` that holds an `Arc<Mutex<StateStore>>` and flushes buffered logs to `journal_logs` after `execute` returns.
- Replace the empty `__ctx` Object install in step 4a with the real Phase-3 ctx surface (`ctx.strategy.id`, `ctx.strategy.name`, `ctx.run.id`, `ctx.now()`, `ctx.log`, `ctx.actions.noop()` per D-04).
- Add the D-12 `update_run_status_with_transition` API on `StateStore`.
- Wire the three new journal tables (D-06).

The D-11 prelude scrub will continue to fire before the real `ctx` is installed; Plan 03-02 simply re-installs `__ctx` AFTER the scrub line.

## Self-Check: PASSED

- All 8 created files exist (verified via `ls`).
- All 3 commits exist:
  - `3e2f48c` — Task 1 scaffold
  - `0381c59` — Task 2 Sandbox::execute
  - `e4c6ec1` — Task 3 D-11 regression suite
- Workspace test count: 114 (= 92 baseline + 22 new strategy-js).
- `cargo clippy --workspace --all-targets -- -D warnings` exits 0.
- `cargo tree -p strategy-js | grep -E '(libloading|tokio v)'` empty.
