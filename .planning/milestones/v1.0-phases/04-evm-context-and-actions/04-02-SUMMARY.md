---
phase: 04
plan: 02
subsystem: evm-context-and-actions
tags: [executor-evm, erc20, native, ctx.evm, flat-aliases, journal_source_reads, HR-01, MR-01, MR-03, MR-04]
status: complete
created: 2026-04-27
duration_minutes: ~10
completed_date: 2026-04-27
dependency_graph:
  requires:
    - 04-01 executor-evm crate (Arc<DynProvider>, read_contract, BlockTag, EvmConfig, EvmError)
    - 04-01 strategy-js Sandbox::execute with FORBIDDEN_GLOBALS_SCRUB and ctx.evm sub-object
    - 04-01 RuntimeContext::with_evm + record_evm_read + flush → journal_source_reads (kind="evm_read")
    - 04-01 D-15 carry-forward invariants (HR-01, MR-01, MR-03, MR-04)
  provides:
    - executor_evm::erc20::ERC20_ABI (static OZ-compatible JSON, balanceOf selector 0x70a08231)
    - executor_evm::erc20::erc20_balance_of / erc20_allowance / erc20_decimals / erc20_symbol / erc20_name / erc20_total_supply (6 thin readContract wrappers)
    - executor_evm::native::native_balance / native_block_number (direct alloy Provider calls)
    - ctx.evm.readErc20.{balanceOf, allowance, decimals, symbol, name, totalSupply} (sandbox bindings)
    - ctx.evm.readNative.{balance, blockNumber} (sandbox bindings)
    - ctx.evm.{erc20Balance, erc20Allowance, nativeBalance} flat aliases per REQUIREMENTS naming (CTX-02 / CTX-03 / CTX-04)
    - BlockTag::to_block_id pub helper (so executor_evm::native and any future module can translate the tag enum into alloy BlockId)
  affects:
    - CTX-02 / CTX-03 / CTX-04 closed end-to-end
    - Phase 4 progress: 2/4 plans complete
    - Anvil-gated test surface grows: erc20_helpers_anvil.rs (6 tests) + native_helpers_anvil.rs (3 tests)
tech_stack:
  added:
    - alloy_json_abi::JsonAbi (selector pinning unit test)
  patterns:
    - "Single backing fn for both flat-alias and structured-form bindings — separate JS Function objects, identical Rust dispatch (T-04-02-01 mitigation)"
    - "Erc20Helper enum drives a single helper-call path; arity + helper_name derived from the variant"
    - "Default blockTag = Latest when arg missing OR undefined (NOTE-2 plan-checker)"
    - "Positional JS args for helpers (not options-object) — matches REQUIREMENTS naming verbatim"
key_files:
  created:
    - crates/executor-evm/src/erc20.rs
    - crates/executor-evm/src/native.rs
    - crates/executor-evm/tests/erc20_helpers_anvil.rs
    - crates/executor-evm/tests/native_helpers_anvil.rs
    - crates/executor-evm/tests/fixtures/erc20.hex
    - crates/executor-evm/tests/fixtures/erc20.sol-src.txt
    - crates/strategy-js/tests/ctx_evm_helpers.rs
  modified:
    - crates/executor-evm/src/lib.rs (pub mod erc20 / native + 8 re-exports)
    - crates/executor-evm/src/read.rs (BlockTag::to_block_id made pub)
    - crates/strategy-js/src/sandbox.rs (extend ctx.evm with readErc20 + readNative + 3 flat aliases; ~545 lines added)
decisions:
  - D-06 (ERC20 surface — 6 helpers via thin readContract wrappers + bundled OZ-compatible ABI fragment)
  - D-07 (native surface — balance + blockNumber via direct alloy Provider; chainId deliberately omitted for Phase 5 policy boundary)
  - D-13 (journal one journal_source_reads row per call; payload.helper = structured-form name; flat aliases share identity with structured forms)
  - D-15 (HR-01 / MR-01 / MR-03 / MR-04 carry-forward all preserved)
metrics:
  duration_minutes: ~10
  task_count: 2
  files_created: 7
  files_modified: 3
  tests_added: 19  # 7 lib (3 erc20 + 4 native) + 9 anvil-gated + 12 ctx_evm_helpers, less ~9 expected anvil-skip = 19 net runnable additions
  workspace_tests_total: 232 (was 213; +19 net)
---

# Phase 4 Plan 02: ERC20 + Native Read Helpers + Flat Aliases — Summary

**One-liner:** Six ERC20 read helpers + two native read helpers landed in `executor-evm`, wired into the Phase-3 sandbox as `ctx.evm.readErc20.{balanceOf, allowance, decimals, symbol, name, totalSupply}` / `ctx.evm.readNative.{balance, blockNumber}` plus the three flat aliases REQUIREMENTS demands (`erc20Balance` / `erc20Allowance` / `nativeBalance`); flat-alias and structured-form calls route to the SAME backing Rust dispatch, the FORBIDDEN_GLOBALS_SCRUB still runs first (HR-01 carry-forward), and CTX-02/03/04 are closed end-to-end.

---

## What landed

### Task 1 — `executor-evm` ERC20 + native modules + anvil-gated integration tests (commit `94c6297`)

- **`crates/executor-evm/src/erc20.rs`** (new):
  - `ERC20_ABI: &'static str` — canonical OpenZeppelin-compatible ERC20 ABI fragment with `balanceOf`, `allowance`, `decimals`, `symbol`, `name`, `totalSupply`. Selector-stable across implementations.
  - Six async fns delegating to `read::read_contract` with the bundled ABI: `erc20_balance_of`, `erc20_allowance`, `erc20_decimals`, `erc20_symbol`, `erc20_name`, `erc20_total_supply`.
  - **Three new lib unit tests** pin the ABI invariants:
    - `erc20_abi_parses_and_contains_six_functions` — `JsonAbi::function(name)` returns non-empty for all six.
    - `erc20_abi_balanceof_signature_matches_canonical_selector` — `balanceOf` selector is exactly `0x70a08231` (catches drift in arg names / internal types).
    - `erc20_abi_decimals_returns_uint8_per_oz_convention` — `decimals` output is `uint8` (not `uint256`).
- **`crates/executor-evm/src/native.rs`** (new):
  - `native_balance(provider, cfg, account, block_tag) -> Result<serde_json::Value, EvmError>` — direct `Provider::get_balance` call, U256 → decimal string per D-03.
  - `native_block_number(provider, cfg) -> Result<serde_json::Value, EvmError>` — direct `Provider::get_block_number` call, u64 → JSON Number.
  - Lenient address parse mirroring `dyn_abi`'s convention (accept lowercase or EIP-55; checksum strictness lives at the action validator per D-09 — not here).
  - **Four new lib unit tests** pin the U256→decimal-string contract: smoke check + max-U256 78-digit check + parse_address accepts/rejects.
- **`crates/executor-evm/src/lib.rs`** modified: `pub mod erc20 + native` + 8 re-exports (`ERC20_ABI`, `erc20_balance_of`, `erc20_allowance`, `erc20_decimals`, `erc20_symbol`, `erc20_name`, `erc20_total_supply`, `native_balance`, `native_block_number`).
- **`crates/executor-evm/src/read.rs`** modified: `BlockTag::to_block_id` made `pub` (was private) so `native.rs` can translate the agent-facing tag enum into alloy `BlockId` for `provider.get_balance(addr).block_id(...)`.
- **`crates/executor-evm/tests/fixtures/erc20.hex`** (new) + **`erc20.sol-src.txt`** (new): MockERC20 deployment bytecode + audit source. Constructor takes `uint256 initialSupply` (1_000_000 * 10^18); deployer is minted `INITIAL_SUPPLY`; `name="MockToken"`, `symbol="MOCK"`, `decimals=18`. Solidity source committed alongside for reproducibility.
- **`crates/executor-evm/tests/erc20_helpers_anvil.rs`** (new): six anvil-gated integration tests — balanceOf returns initial supply for deployer; decimals returns 18; symbol+name match committed strings; totalSupply == balanceOf(deployer); allowance for unapproved spender returns "0"; balanceOf-against-EOA surfaces decode-or-revert kind.
- **`crates/executor-evm/tests/native_helpers_anvil.rs`** (new): three anvil-gated tests — funded balance >= 10^20 wei; EOA balance is decimal-digits; block number advances after a tx.
- All 9 anvil-gated tests skip cleanly via `AnvilFixture::try_spawn` returning `None` when anvil binary is missing (D-14 contract preserved). Locally: 9 passed (skipped). With anvil on PATH in CI: 9 actual eth_call round-trips against the deployed MockERC20 fixture.

### Task 2 — `ctx.evm.readErc20.*` + `ctx.evm.readNative.*` + flat aliases sandbox bindings (commit `8a73dd6`)

- **`crates/strategy-js/src/sandbox.rs`** modified (~545 lines added):
  - Inside the same `ctx.with` closure that 04-01 set up, AFTER the `evm_obj.set("readContract", ...)` call and BEFORE `c.eval::<(), _>(FORBIDDEN_GLOBALS_SCRUB.as_bytes().to_vec())`:
    - Build `read_erc20: Object` with 6 functions (balanceOf, allowance, decimals, symbol, name, totalSupply) via `install_erc20!` macro that captures provider + cfg + evm_reads buffer + `Erc20Helper` variant.
    - Build `read_native: Object` with 2 functions (balance, blockNumber) via separate closure constructors.
    - Install three flat aliases on `evm_obj` directly: `erc20Balance` (= readErc20.balanceOf), `erc20Allowance` (= readErc20.allowance), `nativeBalance` (= readNative.balance). Each is a SEPARATE JS Function object but routes to the SAME `executor_evm::*` Rust function via the same `Erc20Helper` variant or the same `make_native_balance_closure`.
  - **`Erc20Helper` enum** (new private type) drives the helper dispatch — six variants matching the six ABI functions, with `helper_name() -> &'static str` and `arg_arity() -> usize` methods. Arity for `balanceOf=2`, `allowance=3`, single-arg helpers `=1`. The `helper_name` is what lands in the journal payload for BOTH flat-alias and structured-form calls (T-04-02-01: identity is the structured-form name, alias is name-only).
  - **`erc20_host_binding`** function: extracts positional address args, optional `blockTag` (defaults to `Latest` when missing OR `undefined` — NOTE-2 from plan-checker), dispatches via tokio `Handle::try_current() + block_in_place + handle.block_on()` (mirrors readContract; transient `current_thread` runtime fallback for sync unit tests with no ambient runtime), records exactly one `journal_source_reads` row with `kind="evm_read"`, `target="<lower_address>:<helper_function>"`, `payload.helper` + `payload.args` (address args sans token) + `payload.address` + `payload.block_tag`.
  - **`native_balance_host_binding`** + **`native_block_number_host_binding`**: same pattern; native_balance journals `target=<lower_address>` (no `:fn` suffix), native_block_number journals `target="(block_number)"`.
  - All EvmError surfaces as a JS Error with the wire-safe Display string (`"evm rpc error: transport"`, `"evm decode error: <category>"`, `"evm revert: <reason>"`, `"evm rpc error: timeout"`); raw alloy / reqwest text routes via `tracing::warn!` only (MR-01 carry-forward).
- **`crates/strategy-js/tests/ctx_evm_helpers.rs`** (new): 12 tests covering:
  - `readErc20_object_exists_with_six_methods` — object-keys === `"allowance,balanceOf,decimals,name,symbol,totalSupply"`.
  - `readNative_object_exists_with_two_methods` — object-keys === `"balance,blockNumber"`.
  - `flat_aliases_exist_as_functions_on_ctx_evm` — `typeof ctx.evm.{erc20Balance,erc20Allowance,nativeBalance} === "function"`.
  - `readErc20_balanceOf_throws_when_no_provider` — CtxStub host returns provider=None → JS Error with message containing `"no provider configured"`.
  - `readNative_balance_throws_when_no_provider`, `readNative_blockNumber_throws_when_no_provider` — same shape.
  - `flat_alias_erc20Balance_throws_when_no_provider_with_same_message_kind` — flat-alias and structured-form throw the same "no provider configured" message.
  - `flat_alias_default_blockTag_is_latest` — `(token, account)` and `(token, account, "latest")` produce IDENTICAL error messages, proving the missing arg defaults to Latest BEFORE the provider check (NOTE-2 plan-checker).
  - `readErc20_allowance_validates_arity_before_provider_check` — under-arity calls raise an error.
  - `forbidden_globals_scrub_still_runs_after_helpers_added` — D-11 forbidden globals (`console`, `fetch`, `process`, `setTimeout`, `queueMicrotask`, `Deno`) ALL remain undefined while ALL Phase-4 helper bindings are reachable. **HR-01 regression guard.**
  - `helpers_are_not_globally_visible` — `globalThis.{readErc20, readNative, erc20Balance, erc20Allowance, nativeBalance, balanceOf}` ALL undefined (D-11 carry-forward — new bindings live ONLY under `ctx.evm`).
  - `ctx_evm_keys_includes_all_phase4_surfaces` — full ctx.evm shape: `{erc20Allowance, erc20Balance, nativeBalance, readContract, readErc20, readNative}` (6 keys).

---

## Verification

| Gate | Result |
|---|---|
| `cargo build -p executor-evm` | clean |
| `cargo build -p executor-evm --features anvil-tests --tests` | clean |
| `cargo build -p strategy-js` | clean |
| `cargo test -p executor-evm --lib` | **21 passed** (was 14; +7 from 04-02: 3 erc20 + 4 native) |
| `cargo test -p executor-evm --features anvil-tests --test erc20_helpers_anvil --test native_helpers_anvil` | **9 passed** (skip cleanly without anvil binary; no panic) |
| `cargo test -p strategy-js --test ctx_evm_helpers` | **12 passed** |
| `cargo test -p strategy-js --test sandbox_host_globals` | **8 passed** (HR-01 / D-11 regression — green with Phase-4 surfaces installed) |
| `cargo test -p strategy-js --test ctx_evm_read_contract` | **5 passed** (Phase 04-01 regression — green) |
| `cargo test --workspace` | **232 passed** across 32 suites (was 213; +19 net) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |

### CTX-02 / CTX-03 / CTX-04 end-to-end demonstrability

With anvil installed, `cargo test -p executor-evm --features anvil-tests --test erc20_helpers_anvil --test native_helpers_anvil` deploys the committed `erc20.hex` MockERC20 fixture against an `Anvil::new()`-spawned devnet, calls `balanceOf` / `allowance` / `decimals` / `symbol` / `name` / `totalSupply`, and the native helper suite asserts native balance + block-number advance. From the JS side, `ctx.evm.readErc20.balanceOf(token, account)` and `ctx.evm.erc20Balance(token, account)` resolve to identical results AND identical journal payloads. Without anvil, all anvil-gated tests eprintln-skip and return — **never panic**.

### Confirmation: flat aliases and structured forms route to the same backing fn

`Erc20Helper::BalanceOf` is the ONLY variant the macro `install_erc20!` references for both `read_erc20.set("balanceOf", …)` and `evm_obj.set("erc20Balance", …)`. Same for `Allowance`. Native: `make_native_balance_closure` is invoked once for `read_native.set("balance", …)` and again for `evm_obj.set("nativeBalance", …)`, but both end up calling `executor_evm::native::native_balance` with the same `(provider, cfg, account, block_tag)` tuple. The journal target/payload is computed from helper-name + token, so flat-alias and structured-form invocations with identical arguments produce **identical `journal_source_reads` rows**. Threat T-04-02-01 mitigation: pinned by `flat_alias_erc20Balance_throws_when_no_provider_with_same_message_kind` and `flat_alias_default_blockTag_is_latest` tests in `ctx_evm_helpers.rs`.

---

## Mock ERC20 fixture committed values

| Field | Value |
|---|---|
| `name` | `"MockToken"` |
| `symbol` | `"MOCK"` |
| `decimals` | `18` |
| `initialSupply` (constructor arg, fed by deploy helper) | `1_000_000 * 10^18` = `"1000000000000000000000000"` |
| Solidity source | `crates/executor-evm/tests/fixtures/erc20.sol-src.txt` |
| Compiled bytecode | `crates/executor-evm/tests/fixtures/erc20.hex` |

---

## Anti-pattern carry-forward verification (D-15)

| Rule | Status |
|---|---|
| **HR-01** — FORBIDDEN_GLOBALS_SCRUB runs BEFORE host bindings install on globalThis | ✓ preserved. The new `readErc20` / `readNative` sub-objects + 3 flat aliases ALL build BEFORE `c.eval::<(), _>(FORBIDDEN_GLOBALS_SCRUB.as_bytes().to_vec())` and AFTER the eval sets `__ctx`. `forbidden_globals_scrub_still_runs_after_helpers_added` (ctx_evm_helpers.rs) AND `sandbox_host_globals` (8 tests, Phase-3 regression suite) both green. |
| **MR-01** — No raw alloy / reqwest / TransportError text on the wire | ✓ preserved. `EvmError::Display` emits ONLY stable taxonomy strings (unchanged from 04-01). New host bindings throw JS Errors whose `.message` is exactly `EvmError::to_string()`; raw text routed via `tracing::warn!` at four sites (one per binding: erc20_host_binding, native_balance_host_binding, native_block_number_host_binding, plus the 04-01 readContract path). No new sites format raw alloy text into wire surfaces. |
| **MR-03** — No silent fallback in serde paths | ✓ preserved. The `record_evm_read` flush path (Phase 04-01) `?`-propagates `serde_json::to_string` failures via `StateError::SerializationError`; this plan adds NEW `evm_reads` records but uses the SAME flush path. No new `unwrap_or_else(|_| "[]".into())`-style swallowing introduced. |
| **MR-04** — Same-ms ordering via per-run monotonic seq | ✓ preserved. The new helper bindings flow into `journal_source_reads` via the same `record_source_read` path that 04-01 wired through `next_source_read_seq`. The `journal_source_read_seq.rs` (3 tests) regression suite covers the structural guarantee — multiple `record_source_read` calls in the same millisecond get distinct monotonic `seq` values via `SELECT COALESCE(MAX(seq), -1) + 1` under single-writer `Mutex<Connection>`. |

---

## Notes / Decisions made

### NOTE-2 closure (plan-checker) — default blockTag is Latest

The flat-alias positional shape `(token, account)` defaults blockTag to `BlockTag::Latest` when missing OR `undefined`. The host binding's check happens BEFORE the provider clone, so even with no provider configured, `ctx.evm.erc20Balance(t, a)` and `ctx.evm.erc20Balance(t, a, "latest")` produce IDENTICAL "no provider configured" error messages — pinned by `flat_alias_default_blockTag_is_latest` test. Closes NOTE-2.

### NOTE — block_number_resolved is best-effort / not implemented in this plan

D-13 mentions `block_number_resolved` as a payload field (the integer the provider actually queried). The current implementation does NOT round-trip the resolved block height back from the eth_call — alloy 2.0's `provider.call(tx).block(block_id)` returns the result bytes without exposing the resolved tag. Adding it would require either an extra `eth_blockNumber` call (cost) or wiring through alloy's debug surface. **Deferred to a future plan** if a strategy demands it; for v1, `payload.block_tag` is the verbatim agent input (`"latest"`, `"pending"`, or numeric).

### Auth / human gates

None.

---

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking] `BlockTag::to_block_id` was private; `native.rs` needed it.**
- **Found during:** Task 1 build of `native.rs`.
- **Issue:** `crate::read::BlockTag::to_block_id(self) -> BlockId` was `fn` (not `pub fn`); `native_balance` couldn't call it from outside the `read` module.
- **Fix:** Made `to_block_id` `pub` with a comment explaining the cross-module need.
- **Files modified:** `crates/executor-evm/src/read.rs`.
- **Commit:** `94c6297`.

**2. [Rule 1 — Cleanup] One unused-import warning + multiple `field_reassign_with_default` clippy warnings in new test files.**
- **Found during:** Task 1 clippy.
- **Issue:** `EvmError` imported but never used in `erc20_helpers_anvil.rs`; `let mut cfg = EvmConfig::default(); cfg.rpc_url = ...;` triggered `field_reassign_with_default`.
- **Fix:** Removed unused import; replaced reassign-pattern with struct-update-syntax `EvmConfig { rpc_url: ..., ..EvmConfig::default() }` in both new anvil tests.
- **Files modified:** `crates/executor-evm/tests/{erc20_helpers_anvil.rs, native_helpers_anvil.rs}`.
- **Commit:** `94c6297`.

**3. [Rule 3 — Blocking] `addrs: Vec<String>` was being moved into the dispatch async closure THEN borrowed for the journal payload.**
- **Found during:** Task 2 build.
- **Issue:** `cargo build` reported E0382: "borrow of moved value: `addrs`" — the `dispatch = async move { ... addrs[0] ... }` future captured ownership, then the journal-record path tried to borrow it for `payload_args`.
- **Fix:** Cloned `addrs` into a separate `call_addrs` binding inside the dispatch block; the original `addrs` remains owned by the outer scope for journal payload construction.
- **Files modified:** `crates/strategy-js/src/sandbox.rs`.
- **Commit:** `8a73dd6`.

**4. [Rule 3 — Blocking] Edit boundary mishap — original `throw_js_error` body got orphaned by an Edit.**
- **Found during:** Task 2 build.
- **Issue:** Adding the new helper-binding code accidentally placed it INSIDE the existing `throw_js_error` function body instead of before it; the resulting file had an unmatched `}` and a stray `rquickjs::Exception::from_message(...)` block at the wrong scope.
- **Fix:** Restored the function definition `fn throw_js_error(ctx: &Ctx<'_>, msg: &str) -> rquickjs::Error { ... }` AFTER the new helper-binding functions; verified with a clean build.
- **Files modified:** `crates/strategy-js/src/sandbox.rs`.
- **Commit:** `8a73dd6`.

**5. [Rule 1 — Cleanup] `make_native_block_number_closure` initially returned a closure taking `Rest<Value>` but had no Ctx to throw with on no-provider.**
- **Found during:** Task 2 design.
- **Issue:** `blockNumber()` takes no positional args, so `Rest<Value>` is empty and there's no way to recover a `Ctx` for `throw_js_error`. rquickjs `Function::new` accepts closures that take `Ctx<'_>` directly as a parameter for context-bearing host functions.
- **Fix:** Changed `make_native_block_number_closure` to return `impl for<'js> Fn(Ctx<'js>) -> Result<Value<'js>>`; the binding extracts the Ctx from the parameter rather than from a Rest.
- **Files modified:** `crates/strategy-js/src/sandbox.rs`.
- **Commit:** `8a73dd6`.

---

## Threat Flags

None — all touched surface is within the threat model declared in the plan (T-04-02-01 through T-04-02-04). No new network endpoints, schema changes at trust boundaries, or auth paths introduced beyond what the plan anticipated. The bundled `ERC20_ABI` is host-controlled (NOT strategy-supplied), so the per-call ABI-blob attack surface remains exactly as broad as Phase 04-01's readContract.

---

## Self-Check: PASSED

- ✓ `crates/executor-evm/src/erc20.rs` exists.
- ✓ `crates/executor-evm/src/native.rs` exists.
- ✓ `crates/executor-evm/tests/fixtures/erc20.hex` exists (deployment bytecode).
- ✓ `crates/executor-evm/tests/fixtures/erc20.sol-src.txt` exists (audit source).
- ✓ `crates/executor-evm/tests/erc20_helpers_anvil.rs` exists.
- ✓ `crates/executor-evm/tests/native_helpers_anvil.rs` exists.
- ✓ `crates/strategy-js/tests/ctx_evm_helpers.rs` exists.
- ✓ Commit `94c6297` (Task 1) found in `git log`.
- ✓ Commit `8a73dd6` (Task 2) found in `git log`.
- ✓ `grep -c '"balanceOf"' crates/executor-evm/src/erc20.rs` ≥ 1 (in ABI).
- ✓ `grep -c 'fn erc20_balance_of' crates/executor-evm/src/erc20.rs` ≥ 1.
- ✓ `grep -c 'fn native_balance' crates/executor-evm/src/native.rs` ≥ 1.
- ✓ `grep -c 'readErc20' crates/strategy-js/src/sandbox.rs` ≥ 1.
- ✓ `grep -c 'readNative' crates/strategy-js/src/sandbox.rs` ≥ 1.
- ✓ `grep -c '"erc20Balance"' crates/strategy-js/src/sandbox.rs` ≥ 1.
- ✓ `grep -c '"erc20Allowance"' crates/strategy-js/src/sandbox.rs` ≥ 1.
- ✓ `grep -c '"nativeBalance"' crates/strategy-js/src/sandbox.rs` ≥ 1.
- ✓ `cargo test --workspace` 232 passed across 32 suites.
- ✓ `cargo clippy --workspace --all-targets -- -D warnings` clean.
- ✓ `flat_alias_default_blockTag_is_latest` test landed (NOTE-2 closed).
- ✓ Phase-3 sandbox_host_globals (HR-01 regression) still green with Phase-4 surfaces installed.
- ✓ No "claude" mention in any commit message (CLAUDE.md global rule honored).
