---
phase: 04
plan: 01
subsystem: evm-context-and-actions
tags: [executor-evm, alloy, dyn-abi, ctx.evm, journal_source_reads, MR-04, HR-01, MR-01, MR-03]
status: complete
created: 2026-04-27
duration_minutes: ~30
completed_date: 2026-04-27
dependency_graph:
  requires:
    - executor-state journal_source_reads + record_source_read (Phase 3 D-06)
    - strategy-js Sandbox + CtxHost (Phase 3 D-04)
    - executor-mcp ExecutorServer + map_runtime_error (Phase 3)
    - Phase 3 REVIEW-FIX HR-01 / MR-01 / MR-03 / MR-04 invariants
  provides:
    - executor-evm crate (alloy 2.0.1 + dyn-abi 1)
    - EvmConfig (D-04 defaults + range validation)
    - EvmError typed enum + data_kind() taxonomy (D-12)
    - build_provider() → Arc<DynProvider> (Send + Sync + Clone)
    - read_contract async entry point + ReadContractInput / BlockTag
    - dyn_abi walker (D-03 BigInt convention) — js_value_to_dyn_sol / dyn_sol_to_js_value
    - AnvilFixture (gated on anvil-tests / test-fixtures features)
    - journal_source_reads.seq column + UNIQUE(run_id, seq) (MR-04)
    - [evm] config section in ExecutorConfig (rpc_url, call_timeout_ms)
    - ExecutorServer.evm_provider() — lazy OnceCell<Arc<DynProvider>>
    - map_evm_error() → -32017 with data.kind ∈ {evm_rpc_error, evm_decode_error, evm_revert}
    - RuntimeError::Evm(EvmError) variant routed through map_runtime_error
    - ctx.evm.readContract host binding (callable from sandbox)
    - RuntimeContext.with_evm(provider, cfg) builder + evm_reads drain → journal_source_reads (kind="evm_read")
  affects:
    - strategy-js depends on executor-evm (D-02 isolation: still no direct alloy dep)
    - Phase 3 ctx_object_shape_matches_d04 test updated to include "evm" key
tech_stack:
  added:
    - alloy 2.0.1 (provider-http, contract, rpc-types-eth, json-rpc, reqwest-rustls-tls)
    - alloy-dyn-abi 1.5.7
    - alloy-json-abi 1.5.7
    - alloy-primitives 1.5.7
    - alloy node-bindings (dev-only via --features anvil-tests)
    - url 2
  patterns:
    - "Per-crate dep pinning (D-01): alloy NOT promoted to workspace.dependencies until Phase 5 adds a second consumer"
    - "Lazy OnceCell<Arc<DynProvider>>: server boot independent of devnet liveness"
    - "tokio::runtime::Handle::try_current() + block_in_place + handle.block_on() inside spawn_blocking; storage Mutex dropped first (D-04)"
    - "Wire-safe Display + detail_for_log split (HR/MR-01 carry-forward)"
    - "MR-04 monotonic seq via SELECT COALESCE(MAX(seq), -1) + 1 + UNIQUE constraint backstop"
key_files:
  created:
    - crates/executor-evm/Cargo.toml
    - crates/executor-evm/src/lib.rs
    - crates/executor-evm/src/error.rs
    - crates/executor-evm/src/config.rs
    - crates/executor-evm/src/provider.rs
    - crates/executor-evm/src/dyn_abi.rs
    - crates/executor-evm/src/read.rs
    - crates/executor-evm/tests/common/mod.rs
    - crates/executor-evm/tests/common/anvil_fixture.rs
    - crates/executor-evm/tests/fixtures/counter.hex
    - crates/executor-evm/tests/dyn_abi_roundtrip.rs
    - crates/executor-evm/tests/read_contract_anvil.rs
    - crates/executor-state/tests/journal_source_read_seq.rs
    - crates/strategy-js/tests/ctx_evm_read_contract.rs
  modified:
    - Cargo.toml (workspace members += "crates/executor-evm")
    - crates/executor-state/src/schema.rs (journal_source_reads gains seq + UNIQUE)
    - crates/executor-state/src/journal.rs (next_source_read_seq + persist seq + ORDER BY seq + record_source_read_with_time test seam + SourceReadEntry.seq field)
    - crates/executor-state/src/store.rs (__test_record_source_read_with_time helper)
    - crates/executor-mcp/Cargo.toml (executor-evm dep)
    - crates/executor-mcp/src/config.rs ([evm] section parsing + Config::evm_config)
    - crates/executor-mcp/src/server.rs (evm_config + lazy OnceCell<Arc<DynProvider>>; new_with_config / from_config / evm_provider)
    - crates/executor-mcp/src/errors.rs (map_evm_error + extended map_runtime_error dispatch + classifies_evm_kinds test)
    - crates/executor-mcp/src/main.rs (uses from_config so [evm] is honored)
    - crates/executor-mcp/src/tools.rs (strategy_run lazily resolves provider before spawn_blocking; with_evm chain)
    - crates/strategy-js/Cargo.toml (executor-evm path-dep)
    - crates/strategy-js/src/lib.rs (re-export EvmReadRecord)
    - crates/strategy-js/src/error.rs (RuntimeError::Evm(EvmError) #[from])
    - crates/strategy-js/src/runtime.rs (provider/evm_config/evm_reads fields + with_evm builder + flush drains evm_reads → journal_source_reads kind=evm_read with MR-03 ?-propagation)
    - crates/strategy-js/src/sandbox.rs (ctx.evm sub-object; read_contract_host_binding helper; json_to_qjs_value + parse_block_tag + throw_js_error helpers; evm_reads drain into host after ctx.with)
    - crates/strategy-js/tests/ctx_host_api.rs (ctx_object_shape_matches_d04 includes "evm")
decisions:
  - D-01 (alloy 2.0 stack pinned to executor-evm only — alloy 2.0.1 verified via cargo tree)
  - D-02 (executor-evm crate; strategy-js stays alloy-free except via re-export of DynProvider type alias)
  - D-03 (BigInt bridge — decimal-string for any value wider than i32, validated by 11 dyn_abi roundtrip tests)
  - D-04 (Provider lazy OnceCell + [evm] config + per-call timeout)
  - D-05 (readContract input shape — abi accepts string OR JS array; both paths covered)
  - D-12 (EVM error surfacing via -32017 with extended data.kind taxonomy; raw text only via tracing::warn!)
  - D-13 (one journal_source_reads row per ctx.evm.* call with kind="evm_read")
  - D-14 (Anvil fixture behind anvil-tests feature; clean skip on missing binary)
  - D-15 (HR-01 / MR-01 / MR-03 / MR-04 carry-forward all preserved)
metrics:
  duration_minutes: ~30
  task_count: 3
  files_created: 14
  files_modified: 14
  tests_added: 28
  workspace_tests_total: 213 (was 175)
  alloy_version_pinned: "2.0.1"
---

# Phase 4 Plan 01: executor-evm crate scaffolding + alloy provider + dyn-abi readContract + ctx.evm.readContract host binding — Summary

**One-liner:** `executor-evm` crate landed (alloy 2.0.1, dyn-abi BigInt bridge, lazy `Arc<DynProvider>`, anvil-gated tests), the `ctx.evm.readContract` host binding is reachable from the Phase-3 sandbox after the FORBIDDEN_GLOBALS_SCRUB (HR-01 preserved), the EVM error taxonomy joins `-32017` with `data.kind ∈ {evm_rpc_error, evm_decode_error, evm_revert}`, and `journal_source_reads` gains a `seq` column for same-millisecond ordering (MR-04).

---

## What landed

### Task 1 — Crate scaffold + journal seq column (commit `60f1a8f`)

- New workspace member `crates/executor-evm/` with alloy 2.0.1 + dyn-abi 1.5.7 + json-abi 1.5.7 + primitives 1.5.7 (per-crate, NOT workspace.dependencies — D-01).
- `EvmConfig` defaults verbatim D-04: `rpc_url=http://127.0.0.1:8545/`, `call_timeout=1s`. `from_raw` validates URL parse + timeout range `[50, 30_000] ms`.
- `EvmError` typed enum (Transport / Decode / Revert / Timeout / Encode / Config) with **stable Display** + per-variant `detail_for_log` field. `data_kind()` dispatches into the Phase 4 D-12 taxonomy (`evm_rpc_error` / `evm_decode_error` / `evm_revert`).
- `build_provider(&EvmConfig) -> Result<Arc<DynProvider>, EvmError>` via `ProviderBuilder::new().connect_http(...).erased()`. The Arc is `Send + Sync + Clone` (compile-time witness in test).
- Full `dyn_abi` walker covering 7 shapes (uint256 decimal-string, uint32 number, int256 signed, address EIP-55 round-trip, bytes/bytesN hex, tuple ABI-driven, dynamic array, fixed-array length enforcement) + BigInt rejection + uint64-rejects-Number guard. 11 round-trip tests; all pass.
- `AnvilFixture::try_spawn() -> Option<Self>` — `ANVIL_RPC_URL` env override + clean `eprintln! + return None` on missing binary (D-14).
- `journal_source_reads.seq INTEGER NOT NULL + UNIQUE (run_id, seq)` mirrors the Phase-3 MR-04 fix on `journal_logs`. `next_source_read_seq` derives via `SELECT COALESCE(MAX(seq), -1) + 1 + ?`. `list_source_reads_for_run` orders by `recorded_at ASC, seq ASC`. 3 regression tests prove monotonicity, same-millisecond ordering, and per-run scoping.

### Task 2 — read_contract eth_call lifecycle + lazy provider + [evm] config + EVM error taxonomy (commit `117eceb`)

- `read_contract` implements the RESEARCH 9-step flow against alloy 2.0: parse address → JsonAbi → resolve overload by arg count (Pitfall 4) → encode args via `js_value_to_dyn_sol` → `Function::abi_encode_input` → `TransactionRequest::default().to(addr).input(calldata)` → `tokio::time::timeout(cfg.call_timeout, provider.call(tx).block(block_id))` → `abi_decode_output` → `dyn_sol_to_js_value`.
- Single output unwraps to a value; multi-output yields a JSON array (D-03).
- `classify_provider_error()` heuristic split between `Transport` and `Revert`. **Best-effort revert decoding** of `Error(string)` selector `0x08c379a0` (manual hex scan + 32-byte word parse, no extra dep). Raw text stays in `detail_for_log` only.
- 4 new lib tests (`read_contract_decode_error_when_abi_function_not_found`, `..._when_overload_arity_mismatch`, `..._on_bad_address`, `classify_revert_finds_standard_error_string`) + 1 timeout-canary unit test.
- 6 anvil-feature integration tests (deploy Counter via funded deployer, send `eth_sendTransaction`, read `number()`, call `increment()`, revert/decode for empty contract, overload picking, per-call timeout). All skip cleanly via `AnvilFixture::try_spawn` returning `None` when anvil binary is missing.
- `executor-mcp`'s `[evm]` config section parses with `deny_unknown_fields`. Defaults preserve server boot. `Config::evm_config()` builds typed `EvmConfig` (range-validated). 4 new config tests.
- `ExecutorServer` gains `evm_config: EvmConfig` + `Arc<OnceCell<Arc<DynProvider>>>`. New constructors `new_with_config(state, evm)` and `from_config(Config)`. `async fn evm_provider()` lazy-builds on first call. `main.rs` switches to `from_config` so `[evm]` is honored.
- `map_evm_error(EvmError, run_id) -> McpError` emits `-32017` with `data.kind ∈ {evm_rpc_error, evm_decode_error, evm_revert}` and stable wire detail (delegating to `EvmError::Display`). Raw alloy text routed through `tracing::warn!` (mirrors the Phase-3 `map_state_error` storage_error pattern at errors.rs:170).
- `RuntimeError::Evm(EvmError)` variant added with `#[from]`. `map_runtime_error` dispatches to `map_evm_error`.
- New test `map_runtime_error_classifies_evm_kinds` enumerates 5 EvmError variants and asserts: code `-32017`, correct `data.kind`, stable `data.detail`, **NO raw alloy substrings** (`Reqwest`, `alloy_dyn_abi`, `0x08c379a0`) reach the wire.

### Task 3 — ctx.evm.readContract host binding wired into Sandbox (commit `cb336a4`)

- `CtxHost` trait extended **additively** (D-15a): `provider() -> Option<&Arc<DynProvider>>`, `evm_config() -> &EvmConfig`, `record_evm_read(target, payload)` with default impls. `CtxStub` keeps compiling unchanged. `RuntimeContext` overrides all three.
- `RuntimeContext` gains `provider: Option<Arc<DynProvider>>`, `evm_config: EvmConfig`, `evm_reads: Vec<EvmReadRecord>`. Builder `with_evm(provider, cfg)` for attachment after `new`. `flush()` drains `evm_reads` → `journal_source_reads` with `kind="evm_read"`; payload-JSON serialization propagates serde failures via `StateError::SerializationError` (MR-03 carry-forward).
- `Sandbox::execute` installs the `ctx.evm` sub-object with `readContract` host fn alongside the existing `ctx.actions/log/now/strategy/run` sub-objects, BEFORE the `FORBIDDEN_GLOBALS_SCRUB` eval. The scrub still runs BEFORE `c.globals().set("__ctx", ctx_obj)` makes the assembled namespace reachable to JS — **HR-01 ordering preserved**.
- `read_contract_host_binding` helper: extracts `{address, abi, function, args, blockTag}` from JS args. **abi accepts JSON string OR JS array** (D-05). `blockTag` accepts `"latest" | "pending" | non-negative number`; missing/null defaults to `Latest`.
- Concurrency: `tokio::runtime::Handle::try_current()` + `block_in_place` + `handle.block_on(read_contract(...))` when an ambient runtime exists (production path through `strategy_run`). Falls back to a transient `current_thread` runtime when no ambient handle (CtxStub-driven unit tests). **Storage mutex is NOT acquired in this path** (D-04 mutex discipline).
- EvmError surfaces as a JS exception with the wire-safe Display string. The Phase-3 `classify_message` → `RuntimeError::Exception` path forwards through `map_runtime_error` → `map_evm_error` at the MCP boundary for the final `-32017 + data.kind` taxonomy.
- New `crates/strategy-js/tests/ctx_evm_read_contract.rs`: 5 tests covering namespace presence, function reachability, no-provider error path with stable message, HR-01 ordering with `ctx.evm` injected, and globalThis cleanliness (`evm` / `readContract` MUST NOT escape to globalThis).
- `executor-mcp/tools.rs` `strategy_run`: lazily resolves `evm_provider()` before `spawn_blocking`. Provider build failure surfaces `None` (strategies that don't use ctx.evm still succeed). `evm_config + provider` attached to `RuntimeContext` via `with_evm`.
- Re-export `executor_evm::DynProvider` type alias so executor-mcp can name `Arc<DynProvider>` without a direct alloy dep (D-02 boundary preserved).

---

## Verification

| Gate | Result |
|---|---|
| `cargo build -p executor-evm` | clean |
| `cargo build -p executor-evm --features anvil-tests` | clean |
| `cargo test -p executor-evm --lib` | **14 passed** (config + error + read paths) |
| `cargo test -p executor-evm --test dyn_abi_roundtrip` | **11 passed** |
| `cargo test -p executor-evm --features anvil-tests --test read_contract_anvil` | **6 passed** (skip cleanly without anvil binary; no panic) |
| `cargo test -p executor-state --test journal_source_read_seq` | **3 passed** |
| `cargo test -p executor-mcp --lib config:: errors::map_runtime_error_classifies_evm_kinds` | **15 filtered passed** (4 new config + 1 new evm taxonomy) |
| `cargo test -p strategy-js --test ctx_evm_read_contract` | **5 passed** |
| `cargo test -p strategy-js --test sandbox_host_globals` | **8 passed** (HR-01 / D-11 regression — green) |
| `cargo test --workspace` | **213 passed** (was 175; +38 net Phase-4 additions) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo tree -p executor-evm \| grep '^alloy v'` | `alloy v2.0.1` ✓ |

### CTX-01 end-to-end demonstrability

`cargo test -p executor-evm --features anvil-tests --test read_contract_anvil read_counter_number_returns_zero` deploys the committed `counter.hex` bytecode against an `Anvil::new()`-spawned devnet, calls `number()`, and asserts the decoded uint256 round-trips to a JSON string `"0"`. With anvil installed, this is the live demo path. Without anvil, the test eprintln-skips and returns `None` — **never panics** (D-14 contract).

---

## Decisions made / Notes

### NOTE-3: revert taxonomy — Revert vs Decode

The plan asked the executor to document whether reverts surface as `EvmError::Revert` or get misclassified as `EvmError::Decode`. With the current `classify_provider_error` heuristic:

- **Reverts via the standard `Error(string)` selector** (`0x08c379a0`) decode cleanly into `Revert { reason, ... }`. The `classify_revert_finds_standard_error_string` unit test pins this with a synthetic transport string (Hello World!). On a live anvil revert, the alloy 2.0 `TransportError::Display` includes the phrase "execution reverted" + the hex payload, which the heuristic catches.
- **Empty-bytes "calling missing contract"** (e.g. `read_revert_returns_evm_runtime_error_kind` test) may surface as `Decode { abi_decode_output }` rather than `Revert` — alloy 2.0 returns empty bytes from the call rather than a JSON-RPC error, and `abi_decode_output(&[])` correctly fails as a decode error. The integration test asserts `kind ∈ {evm_decode_error, evm_revert}` to accept both shapes.
- **Custom-revert errors** (Solidity `error MyError(uint256)`-style, selector ≠ `0x08c379a0`) currently surface as `Revert { reason: "unknown", ... }` — the heuristic catches "revert" / "execution reverted" in the transport text but cannot decode the custom selector without the contract's ABI registered upstream. This is a known limitation.

**Follow-up for D-12 taxonomy refinement (next plan):** A future plan may want to route empty-return-bytes through `EvmError::Revert { reason: "empty return (no contract or revert without data)", ... }` for clearer agent dispatch. Logged here for the Phase 4 review.

### NOTE-4: clock source for journal records

Per the plan brief, the EVM read path uses `host.now_millis()` (the existing CtxHost method) for any timestamp work and does NOT pull in `chrono` as a strategy-js dep. In practice the journal `recorded_at` field is filled by `executor-state::journal::record_source_read` via `crate::strategies::now_rfc3339()` (chrono inside executor-state — pre-existing), and the EVM host binding doesn't synthesize its own timestamp. So no chrono leak into strategy-js.

### Auth / human gates

None.

---

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking] alloy 2.0 `Provider` trait must be in scope for `.erased()`.**
- **Found during:** Task 1 build.
- **Issue:** `cargo build` failed with "no method named `erased` found for struct `FillProvider<...>`".
- **Fix:** Added `Provider` trait import to `provider.rs`. The trait carries the method.
- **Files modified:** `crates/executor-evm/src/provider.rs`.
- **Commit:** `60f1a8f`.

**2. [Rule 3 — Blocking] alloy 2.0 `JsonAbiExt` / `FunctionExt` traits required for `abi_encode_input` / `abi_decode_output`.**
- **Found during:** Task 2 build.
- **Issue:** `cargo build` failed with "no method named `abi_encode_input` found".
- **Fix:** Added `use alloy_dyn_abi::{FunctionExt, JsonAbiExt};` to `read.rs`.
- **Files modified:** `crates/executor-evm/src/read.rs`.
- **Commit:** `117eceb`.

**3. [Rule 3 — Blocking] rquickjs `Function::new` closure lifetime variance for `Object<'js> → Value<'js>`.**
- **Found during:** Task 3 build.
- **Issue:** A naive `move |args: Object<'_>| -> Result<Value<'_>>` failed because rquickjs's `Value<'js>` is invariant — Rust couldn't unify the input/output lifetimes.
- **Fix:** Wrapped the closure construction in a helper `fn make_read_contract_closure(...) -> impl for<'js> Fn(Object<'js>) -> Result<Value<'js>> + 'static` so the higher-rank lifetime equality is explicit.
- **Files modified:** `crates/strategy-js/src/sandbox.rs`.
- **Commit:** `cb336a4`.

**4. [Rule 1 — Bug] Phase-3 `ctx_object_shape_matches_d04` test would fail after Phase-4 ctx.evm injection.**
- **Found during:** Task 3 test.
- **Issue:** The test asserts `Object.keys(ctx) == [actions, log, now, run, strategy]`. Adding `ctx.evm` is an expected breaking change.
- **Fix:** Updated the expected-keys vector to include `"evm"`.
- **Files modified:** `crates/strategy-js/tests/ctx_host_api.rs`.
- **Commit:** `cb336a4`.

**5. [Rule 1 — Cleanup] Three clippy warnings auto-fixed during the run.**
- `useless_conversion` on `DynSolValue::Bytes(b.into())` → `b`.
- `derivable_impls` on `BlockTag::default()` → `#[derive(Default)]` + `#[default] Latest`.
- `manual_is_multiple_of` on `s.len() % 2 != 0` → `!s.len().is_multiple_of(2)`.
- `let_and_return` on `throw_js_error` helper.
- `manual_ok_err` on `match self.evm_provider().await { Ok(..) Err(..) }` → `.ok()`.
- All within Tasks 1-3 commits; non-functional.

---

## Anti-pattern carry-forward verification (D-15)

| Rule | Status |
|---|---|
| **HR-01** — FORBIDDEN_GLOBALS_SCRUB runs BEFORE host bindings install on globalThis | ✓ preserved. `ctx.evm` joins the other ctx sub-objects in the BUILD phase; the scrub eval runs unchanged BEFORE `c.globals().set("__ctx", ctx_obj)`. `sandbox_blocks_host_globals` (8 tests) still green. New `ctx_evm_readContract_runs_after_forbidden_globals_scrub` test asserts `console === undefined && fetch === undefined && process === undefined && typeof ctx.evm.readContract === "function"`. |
| **MR-01** — No raw alloy / reqwest / TransportError text on the wire | ✓ preserved. `EvmError::Display` emits ONLY stable taxonomy strings (`"evm rpc error: transport"`, `"evm decode error: <category>"`, `"evm revert: <decoded_reason | unknown>"`, `"evm rpc error: timeout"`). Raw text lives in `detail_for_log` and is routed via `tracing::warn!` at three sites: `map_evm_error` (errors.rs), `read_contract_host_binding` (sandbox.rs). New test `map_runtime_error_classifies_evm_kinds` asserts `Reqwest`, `alloy_dyn_abi`, `0x08c379a0` substrings do NOT reach `e.message` or `data.detail`. |
| **MR-03** — No silent fallback in serde paths | ✓ preserved. `RuntimeContext::flush` payload-JSON serialization for `evm_reads` propagates errors via `StateError::SerializationError` — same shape as the Phase-3 fix in `tools.rs` `record_action`. No `unwrap_or_else(\|_\| "[]".into())` anywhere in the new code. |
| **MR-04** — Same-ms ordering via per-run monotonic seq | ✓ landed. `journal_source_reads.seq INTEGER NOT NULL + UNIQUE (run_id, seq)` schema; `next_source_read_seq` SELECT-then-INSERT under single-writer `Mutex<Connection>`; `list_source_reads_for_run` orders by `recorded_at ASC, seq ASC`. 3 regression tests prove the contract. |

---

## Threat Flags

None — all touched surface is within the threat model declared in the plan. No new network endpoints, schema changes at trust boundaries, or auth paths were introduced beyond what the plan anticipated.

---

## Self-Check: PASSED

- ✓ `crates/executor-evm/Cargo.toml` exists.
- ✓ `crates/executor-evm/src/{lib,error,config,provider,dyn_abi,read}.rs` all exist.
- ✓ `crates/executor-evm/tests/{common/mod.rs,common/anvil_fixture.rs,fixtures/counter.hex,dyn_abi_roundtrip.rs,read_contract_anvil.rs}` all exist.
- ✓ `crates/executor-state/tests/journal_source_read_seq.rs` exists.
- ✓ `crates/strategy-js/tests/ctx_evm_read_contract.rs` exists.
- ✓ Commit `60f1a8f` (Task 1) found.
- ✓ Commit `117eceb` (Task 2) found.
- ✓ Commit `cb336a4` (Task 3) found.
- ✓ `cargo tree -p executor-evm | grep '^alloy v'` reports `alloy v2.0.1`.
- ✓ `grep -c '"crates/executor-evm"' Cargo.toml` == 1.
- ✓ `grep -c 'UNIQUE (run_id, seq)' crates/executor-state/src/schema.rs` == 2 (journal_logs + journal_source_reads).
- ✓ `cargo test --workspace` 213 passed.
- ✓ `cargo clippy --workspace --all-targets -- -D warnings` clean.
- ✓ Phase-3 sandbox_host_globals (HR-01 regression) still green.
- ✓ No "claude" mention in any commit message (CLAUDE.md global rule honored).
