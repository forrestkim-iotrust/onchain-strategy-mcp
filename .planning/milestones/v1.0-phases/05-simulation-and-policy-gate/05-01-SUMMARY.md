---
phase: 05
plan: 01
subsystem: simulation-and-policy-gate
tags: [executor-policy, executor-evm, normalize, encode_call_input, ERC20_WRITE_ABI, MAX_ACTIONS_PER_RUN, BR-02, MR-01, BR-01, D-01, D-02, D-03, D-04, D-12, D-18, D-20]
status: complete
created: 2026-04-27
duration_minutes: ~25
completed_date: 2026-04-27
dependency_graph:
  requires:
    - executor-evm action validators (validate_address / validate_calldata / validate_decimal_amount) — Phase 4 D-09
    - executor-evm dyn_abi js_value_to_dyn_sol walker — Phase 4 D-03
    - executor-evm erc20::ERC20_ABI (read-only sibling) — Phase 4 D-06
    - executor-mcp validate_strategy_output gate (BR-02 dry_run_abi_encode call) — Phase 4 REVIEW-FIX
    - Phase 4 BR-01 / MR-01 / MR-03 / MR-04 / HR-01 / WR-01 / WR-04 carry-forward invariants
  provides:
    - executor-policy crate (alloy-FREE per D-20) — workspace member
    - PolicyError typed enum + wire-safe Display + data_kind() taxonomy
    - PolicyConfig schema (chains / contracts / selectors / native_value / erc20_spend / raw_call) + deny_unknown_fields per struct + deny-all Default
    - Decision + DecisionVerdict alloy-free input/output shapes
    - NormalizedActionKindCopy mirror of executor_evm::normalize::NormalizedActionKind
    - extract_selector + selector_to_hex helpers (POL-03)
    - executor_evm::dyn_abi::encode_call_input(abi, function, args) -> Bytes (D-03 shared encoder)
    - executor_evm::erc20::ERC20_WRITE_ABI (transfer 0xa9059cbb + approve 0x095ea7b3) — D-04
    - executor_evm::normalize::normalize_action(&Action) -> Result<Option<NormalizedAction>, EvmError> per D-02 table
    - executor_evm::normalize::{NormalizedAction, NormalizedActionKind} types
    - executor_mcp::validation::MAX_ACTIONS_PER_RUN: usize = 32 (D-12 / D-18)
    - validate_strategy_output enforces -32018 STRATEGY_INVALID_OUTPUT for actions.len() > 32 (BR-02 carry-forward)
  affects:
    - executor-evm gains pub mod normalize; lib.rs re-exports normalize_action / NormalizedAction / NormalizedActionKind / encode_call_input / ERC20_WRITE_ABI
    - executor-evm/src/action.rs::dry_run_abi_encode now delegates to encode_call_input (refactor; behaviour byte-for-byte preserved; existing 11 lib tests still green)
    - executor-mcp/src/tools.rs::validate_strategy_output gains the cap check BEFORE the per-element kind walk
    - Workspace members list grew from 6 to 7 crates
tech_stack:
  added:
    - executor-policy v0.1.0 (alloy-FREE: executor-core + alloy-primitives + serde + serde_json + thiserror + toml + tracing)
  patterns:
    - "Wire-safe Display + detail_for_log split (Phase-4 BR-01 / MR-01 carry-forward) — PolicyError + executor_evm::EvmError::Encode { category: Cow::Borrowed(...) } in normalize"
    - "Cap-at-output-gate (BR-02 carry-forward) — MAX_ACTIONS_PER_RUN enforced at validate_strategy_output, NOT only at strategy-js builder"
    - "alloy isolation (D-20) — executor-policy NEVER imports the umbrella `alloy` crate; only `alloy-primitives` for Address/U256"
    - "Shared encoder pattern (D-03) — encode_call_input lives in dyn_abi.rs and is consumed by both action::dry_run_abi_encode (discards bytes) and normalize::normalize_contract_call (keeps bytes)"
    - "Sibling-constants invariant (D-04) — ERC20_ABI (read-only) and ERC20_WRITE_ABI (write) are SEPARATE; one is never extended into the other"
key_files:
  created:
    - crates/executor-policy/Cargo.toml
    - crates/executor-policy/src/lib.rs
    - crates/executor-policy/src/error.rs
    - crates/executor-policy/src/model.rs
    - crates/executor-policy/src/decision.rs
    - crates/executor-policy/src/selector.rs
    - crates/executor-evm/src/normalize.rs
    - crates/executor-evm/tests/normalize.rs
    - .planning/phases/05-simulation-and-policy-gate/05-01-SUMMARY.md
  modified:
    - Cargo.toml (workspace members += "crates/executor-policy")
    - Cargo.lock (transitive only — alloy-primitives, toml, etc already in tree)
    - crates/executor-evm/src/lib.rs (pub mod normalize; re-export normalize_action, NormalizedAction, NormalizedActionKind, encode_call_input, ERC20_WRITE_ABI)
    - crates/executor-evm/src/dyn_abi.rs (new pub fn encode_call_input + tests; alloy_json_abi::JsonAbi + JsonAbiExt + Bytes imports added)
    - crates/executor-evm/src/action.rs (dry_run_abi_encode delegates to encode_call_input; unused JsonAbi + decode_err removed)
    - crates/executor-evm/src/erc20.rs (ERC20_WRITE_ABI sibling constant + 4 new tests; existing ERC20_ABI untouched)
    - crates/executor-mcp/src/validation.rs (MAX_ACTIONS_PER_RUN: usize = 32 + max_actions_per_run_constant_is_32 unit test)
    - crates/executor-mcp/src/tools.rs (validate_strategy_output cap check before per-element kind walk)
    - crates/executor-mcp/tests/stdio_handshake.rs (strategy_run_caps_action_array_length_at_32 + strategy_run_accepts_action_array_length_32 boundary regression)
decisions:
  - "encode_call_input lives in dyn_abi.rs (NOT a new module) — it shares the js_value_to_dyn_sol walker; action.rs now contains validators + the Phase-4 wrapper only (D-03)"
  - "validate_abi_size moved INSIDE encode_call_input (not duplicated at call sites) — preserves Phase-4 abi_oversize taxonomy through both dry_run_abi_encode and normalize_contract_call"
  - "normalize::parse_address_field re-wraps Phase-4 EvmError::Encode { category: bad_address } as bad_address_to so the wire detail names the failing field at the normalize layer (vs the inner walker layer)"
  - "Erc20Transfer/Approve calldata uses the original input string (et.to / ea.spender) — both validate_address and js_value_to_dyn_sol accept lowercase + EIP-55, so the original survives parsing without lossy round-trip via Address::Display"
  - "executor-policy crate eagerly defines the full schema (Chains / ContractsAllow / SelectorsAllow / NativeValueCap / Erc20SpendCap / RawCallGate) in 05-01 even though Plan 05-03 lands the load + eval bodies — the scaffolding-only commit is a clean dep boundary"
  - "validate_strategy_output enforces the MAX_ACTIONS cap BEFORE the per-element kind walk so a 1000-element noop array fails fast, NOT after 1000 hashmap lookups"
metrics:
  duration_minutes: ~25
  task_count: 3
  files_created: 9
  files_modified: 9
  workspace_tests_before: 349
  workspace_tests_after: 388
  net_test_delta: +39
  clippy_strict: pass
  alloy_isolation_lines: 0
  policy_crate_dep_count: 7
---

# Phase 05 Plan 01: executor-policy Crate Scaffolding + Action -> TxRequest Normalize + Shared encode_call_input + ERC20_WRITE_ABI Summary

Phase 5 wave 1 plumbing landed: a new alloy-FREE `executor-policy` crate scaffolds the policy DSL surface (PolicyError + PolicyConfig + Decision + selector helpers); `executor-evm` gains a `normalize` layer that converts each Phase-4 `Action` variant into a `TransactionRequest` per the D-02 table; the Phase-4 `dry_run_abi_encode` was refactored to delegate to a new shared `encode_call_input` that returns the encoded `Bytes` (so `normalize_contract_call` and the JSON-output gate share the same encoder); `ERC20_WRITE_ABI` was added as a sibling of `ERC20_ABI` with selector goldens; and `MAX_ACTIONS_PER_RUN = 32` is now enforced at `validate_strategy_output` with a `-32018` stdio regression. EXE-01 and EXE-02 are end-to-end demonstrable.

## What Shipped

### Task 1 — executor-policy crate scaffolding + ERC20_WRITE_ABI + extract encode_call_input

- New workspace member `crates/executor-policy/` (alloy-FREE per D-01 / D-20). `cargo tree -p executor-policy --depth 1 | grep -E '^alloy '` returns 0 lines; only `alloy-primitives` appears as a transitive dep via `executor-core`. The crate's `Cargo.toml` carries a comment block forbidding the umbrella `alloy` dep + the reverse `executor-evm -> executor-policy` dep.
- `executor_policy::error::PolicyError` mirrors Phase 4 `EvmError`: wire-safe `Display` (`"policy violation: …"` / `"policy config error: …"` / `"policy io error"`); raw toml / serde / fs text routes through `detail_for_log` only. `data_kind()` returns the stable `policy_not_loaded` / `policy_config_error` / `policy_violation` taxonomy.
- `executor_policy::model::PolicyConfig` declares the 6 dimensions (`chains` / `contracts` / `selectors` / `native_value` / `erc20_spend` / `raw_call`) with `#[serde(deny_unknown_fields)]` per struct; `Default::default()` is deny-all (empty allowlists, `raw_call.allow_global = false`). Plan 05-03 will land the `load` + `eval` bodies; this commit is scaffolding-only.
- `executor_policy::decision::{Decision, DecisionVerdict, NormalizedActionKindCopy}` — alloy-free input/output shapes for the evaluator. The orchestrator (Plan 05-04) will map `executor_evm::normalize::NormalizedActionKind` -> `NormalizedActionKindCopy` 1:1.
- `executor_policy::selector::{extract_selector, selector_to_hex}` — POL-03 helpers; `extract_selector` returns `None` for `< 4 bytes` (P-4).
- `executor_evm::dyn_abi::encode_call_input(abi, function, args) -> Result<Bytes, EvmError>` is the new pub shared encoder. The full Phase-4 sequence (validate_abi_size -> JsonAbi parse -> overload resolution -> arg walk via `js_value_to_dyn_sol` -> `Function::abi_encode_input`) lives here; the encoded `Bytes` are returned (kept by normalize, discarded by `dry_run_abi_encode`). All 6 stable encode/decode error categories (`abi_oversize` / `abi_parse` / `abi_function_missing` / `abi_arg_count` / `abi_type_parse` / `abi_encode_input`) propagate unchanged.
- `executor_evm::action::dry_run_abi_encode` now reads: `let _bytes = crate::dyn_abi::encode_call_input(abi, function, args)?; Ok(())`. Existing 15+ Phase-4 negative-grid tests in `action::tests` stay green byte-for-byte.
- `executor_evm::erc20::ERC20_WRITE_ABI` is a new sibling of the read-only `ERC20_ABI`. Bundles `transfer(address,uint256)` (selector `0xa9059cbb`) and `approve(address,uint256)` (selector `0x095ea7b3`). Selector goldens lock both via `encode_call_input(ERC20_WRITE_ABI, ...)`. A regression test asserts `ERC20_ABI` does NOT contain `transfer`/`approve` (sibling-constants invariant — D-04).
- Re-exports added to `executor-evm/src/lib.rs`: `encode_call_input` and `ERC20_WRITE_ABI`.

**Tests:** `cargo test -p executor-policy --lib` 13 passed (5 error + 4 model + 2 decision + 4 selector); `cargo test -p executor-evm --lib` 63 passed (was 55; +8: 4 encode_call_input + 4 ERC20_WRITE_ABI). `cargo tree -p executor-policy | grep '^alloy '` returns 0 lines (D-20 verified).

**Commit:** `3b215d8` `feat(05-01): scaffold executor-policy crate (alloy-free) + ERC20_WRITE_ABI + extract encode_call_input shared encoder`

### Task 2 — executor-evm normalize: Action -> NormalizedAction per D-02

- New `crates/executor-evm/src/normalize.rs` with the top-level dispatcher `normalize_action(&Action) -> Result<Option<NormalizedAction>, EvmError>`. `Noop` returns `Ok(None)`; the 5 emitting variants return `Ok(Some(NormalizedAction { tx, source, selector, native_value, erc20_amount }))`.
- Per-variant bodies follow the D-02 table exactly:
  - **ContractCall** — `tx.data` from `encode_call_input(abi, function, args)`; `selector` from first 4 bytes; `native_value = U256::from_str(value)`; `erc20_amount = None`.
  - **RawCall** — `tx.data` from `validate_calldata`; `selector` is `Some` only for `>= 4 bytes` (P-4); POL-06 raw_call gate (Plan 05-03) still applies regardless.
  - **Erc20Transfer** — `tx.data` from `encode_call_input(ERC20_WRITE_ABI, "transfer", [to, amount])`; selector locked to `0xa9059cbb` via `debug_assert_eq`; `tx.value = U256::ZERO`; `erc20_amount = Some(amount)`.
  - **Erc20Approve** — symmetric to transfer with selector `0x095ea7b3`.
  - **NativeTransfer** — empty `tx.data`; full `value`; `selector = None`.
- `tx.gas` / `tx.nonce` / `tx.chain_id` are intentionally NOT set in 05-01 — Phase 6 owns signer-side completion. `tx.from` is also unset (Plan 05-02 owns the simulator's `from_address` injection).
- Bad-input paths return typed `EvmError::Encode { category: Cow::Borrowed(...) }` with new normalize-side categories `bad_address_to` / `bad_decimal_value` / `bad_calldata`. The wire `Display` always starts with `"evm encode error"` (BR-01) and the raw input never leaks (MR-01) — pinned by the `*_bad_address_returns_evm_encode_error` and `*_bad_decimal_value_returns_evm_encode_error` tests.
- Re-exports added to `executor-evm/src/lib.rs`: `normalize_action`, `NormalizedAction`, `NormalizedActionKind`.

**Tests:** `cargo test -p executor-evm --test normalize` 11 passed:
1. `noop_returns_none`
2. `contract_call_normalizes_to_tx_with_encoded_calldata` (selector golden `0xd09de08a`)
3. `contract_call_with_value_propagates_u256` (1 ETH round-trip)
4. `raw_call_with_full_calldata_extracts_selector` (`0xa9059cbb` + 32-byte tail)
5. `raw_call_with_short_calldata_has_none_selector` (`"0x"` and `"0x1234"` → None)
6. `erc20_transfer_normalizes_with_a9059cbb_selector`
7. `erc20_approve_normalizes_with_095ea7b3_selector`
8. `native_transfer_has_empty_data_and_full_value` (5 ETH round-trip)
9. `contract_call_bad_address_returns_evm_encode_error` (BR-01 + MR-01)
10. `native_transfer_bad_decimal_value_returns_evm_encode_error` (BR-01 + MR-01)
11. `contract_call_calldata_is_deterministic` (regression lock)

**Commit:** `6b90f6a` `feat(05-01): Action -> NormalizedAction normalize layer (per-variant table per D-02; reuses encode_call_input + ERC20_WRITE_ABI)`

### Task 3 — MAX_ACTIONS_PER_RUN = 32 cap at validate_strategy_output

- `executor_mcp::validation::MAX_ACTIONS_PER_RUN: usize = 32` is the new constant sibling of `MAX_TAGS = 16` (D-12 / D-18 numeric lock).
- `executor_mcp::tools::validate_strategy_output` gains a length check INSIDE the `Value::Array` arm, BEFORE the per-element `kind` walk. On violation it returns `Err(format!("actions length {} exceeds MAX_ACTIONS_PER_RUN {}", items.len(), MAX_ACTIONS_PER_RUN))` which routes through the existing `strategy_invalid_output(detail, ...)` factory → `-32018` on the wire. NO new wire-code factory was needed (P-6 — `-32018` already does Action[] cap rejection; semantics not moved to `-32017`).
- The cap fires fast (before serde deserialisation + Phase-4 BR-02 dry-run encode walk), so a strategy returning 10_000 noops is rejected at `O(1)` instead of `O(n)`.

**Tests:**
- Lib: `validation::tests::max_actions_per_run_constant_is_32`.
- Stdio: `strategy_run_caps_action_array_length_at_32` — register `(ctx) => Array.from({length: 33}, () => ({kind:'noop'}))`; assert `-32018` with `data.code = "strategy_invalid_output"` and detail containing both `"33"` and `"MAX_ACTIONS_PER_RUN 32"`.
- Stdio boundary: `strategy_run_accepts_action_array_length_32` — exactly 32 noops still pass (the cap is `>` not `>=`).

**Commit:** `dd78e27` `feat(05-01): MAX_ACTIONS_PER_RUN=32 cap at validate_strategy_output + stdio regression (D-12 / BR-02 carry-forward)`

## Cross-Plan Exports

**Plan 05-02 (simulate) consumes:**
- `executor_evm::normalize::{normalize_action, NormalizedAction, NormalizedActionKind}`
- `executor_evm::dyn_abi::encode_call_input`
- `executor_evm::erc20::ERC20_WRITE_ABI`

**Plan 05-03 (policy load + eval) consumes:**
- `executor_policy::{PolicyConfig, PolicyError, Decision, DecisionVerdict, NormalizedActionKindCopy, extract_selector, selector_to_hex}`

**Plan 05-04 (orchestrator wiring) consumes:**
- `executor_mcp::validation::MAX_ACTIONS_PER_RUN`
- All of the above transitively.

## Threat Surface Disposition (per plan threat_model)

| Threat ID  | Disposition  | Verification |
|------------|--------------|--------------|
| T-05-01-01 | mitigated    | `strategy_run_caps_action_array_length_at_32` stdio regression — 33-action strategy → -32018 before serde walks the array. |
| T-05-01-02 | mitigated    | `?`-propagated error categories on every step inside `encode_call_input`; refactor preserves Phase-4 negative-grid byte-for-byte. |
| T-05-01-03 | mitigated    | `contract_call_bad_address_returns_evm_encode_error` + `native_transfer_bad_decimal_value_returns_evm_encode_error` assert wire-safe Display + no raw input leak. |
| T-05-01-04 | mitigated    | `erc20_write_abi_transfer_selector_is_a9059cbb` + `erc20_write_abi_approve_selector_is_095ea7b3` lock both selectors byte-for-byte. |
| T-05-01-05 | accepted     | RawCall ≥4-byte selector extraction is the intended POL-03 input; selectors are public protocol data. |
| T-05-01-06 | mitigated    | `raw_call_with_short_calldata_has_none_selector` confirms `selector = None`; POL-06 raw_call gate (Plan 05-03) handles defense-in-depth. |
| T-05-01-07 | mitigated    | `dry_run_abi_encode` is now a 1-line delegate to `encode_call_input`; no path divergence possible. |

## Carry-Forward Compliance (Phase 3 + Phase 4 anti-pattern lattice)

| Invariant | Plan 05-01 status |
|-----------|------------------|
| HR-01 (forbidden-globals scrub) | Phase 5 adds NO new `ctx.*` surface; scrub site untouched; `cargo test -p strategy-js sandbox_blocks_host_globals` still green (2 passed). |
| MR-01 (no raw alloy/serde/toml on the wire) | `PolicyError::Display` + `EvmError::Encode { category }` carry stable taxonomy only; raw text -> `detail_for_log` -> `tracing::warn!`. Verified by `display_strings_are_stable_and_wire_safe` (PolicyError) + 2 normalize negative tests. |
| MR-03 (no silent serde fallback) | `encode_call_input` and normalize use `?`-propagation throughout; no `unwrap_or_else(|_| default)` introduced. |
| MR-04 (per-run monotonic seq) | Plan 05-01 adds no journal write paths; `journal_logs` and `journal_source_reads` continue to hold their seq invariants from Phase 3 / Phase 4. |
| BR-01 (stable wire taxonomy survives JS round-trip) | `EvmError::Encode { category: Cow::Borrowed("bad_address_to" / "bad_decimal_value" / "bad_calldata") }`. Display starts with `"evm encode error"` always. |
| BR-02 (cap-at-output-gate) | `MAX_ACTIONS_PER_RUN=32` enforced at `validate_strategy_output`, NOT only at strategy-js builder. Pinned by `strategy_run_caps_action_array_length_at_32` regression. |
| WR-01 (no `block_in_place` from inside `spawn_blocking`) | Plan 05-01 introduces no async work — pure functions only. WR-01 stays clean. |
| WR-04 (sanitize attacker-controllable text) | Not exercised in 05-01 (no revert reasons); Plan 05-02 owns. |

## Deviations from Plan

**None — plan executed exactly as written, with three clarifying refinements that do NOT alter the contract:**

1. **`validate_abi_size` moved INSIDE `encode_call_input`** (not duplicated at call sites). The Phase-4 sequence had `validate_abi_size` as step 1 inside `dry_run_abi_encode`; preserving it inside the extracted `encode_call_input` keeps the Phase-4 `abi_oversize` taxonomy reachable through both the `dry_run_abi_encode` path and the new `normalize_contract_call` path. The Phase-4 `dry_run_abi_encode_fails_on_oversize_abi` test stays green unchanged. (Refines Sub-task 1.9's literal interpretation; preserves the plan's "byte-for-byte equivalent" contract.)

2. **`alloy_dyn_abi::JsonAbiExt` import promoted to top-of-file in `dyn_abi.rs`** — Phase 4 had it as a `use` inside the `dry_run_abi_encode` body. Moving it to the file scope removes the `use` from `action.rs` (which no longer needs it) and keeps the import where the `abi_encode_input` call now lives.

3. **`format!("{recipient:?}")` replaced with `serde_json::Value::String(et.to.clone())`** in `normalize_erc20_transfer` / `normalize_erc20_approve`. The plan flagged this as an executor-time decision ("verify against existing call sites — if alloy's Display produces a checksum form, that's fine"). Passing the original validated input string through directly avoids any lossy Address::Display / Debug round-trip and keeps the test assertions stable. Both `js_value_to_dyn_sol`'s address parser and the recipient pre-validation step accept lowercase + EIP-55, so the contract — `tx.data[..4] == [0xa9, 0x05, 0x9c, 0xbb]` for transfer / `[0x09, 0x5e, 0xa7, 0xb3]` for approve — is unaffected and pinned by 2 selector tests.

## Verification

| Check | Command | Result |
|-------|---------|--------|
| Build (full workspace) | `cargo build --workspace` | clean |
| Tests (full workspace) | `cargo test --workspace` | 388 passed (38 suites; was 349 → +39 net) |
| executor-policy lib | `cargo test -p executor-policy --lib` | 13 passed |
| executor-evm lib | `cargo test -p executor-evm --lib` | 63 passed |
| executor-evm normalize integration | `cargo test -p executor-evm --test normalize` | 11 passed |
| executor-mcp validation lib | `cargo test -p executor-mcp --lib validation` | 15 passed |
| stdio cap regression | `cargo test -p executor-mcp --test stdio_handshake strategy_run_caps_action_array_length_at_32` | 1 passed |
| stdio boundary regression | `cargo test -p executor-mcp --test stdio_handshake strategy_run_accepts_action_array_length_32` | 1 passed |
| HR-01 sandbox regression | `cargo test -p strategy-js sandbox_blocks_host_globals` | 2 passed |
| Clippy strict | `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| D-20 alloy isolation | `cargo tree -p executor-policy --depth 1 \| grep -E '^alloy ' \| wc -l` | `0` |
| D-20 alloy-primitives transitive only | `cargo tree -p executor-policy \| grep 'alloy-primitives'` | `├── alloy-primitives v1.5.7` |

## Files Touched

**Created (9):**
- `crates/executor-policy/Cargo.toml`
- `crates/executor-policy/src/lib.rs`
- `crates/executor-policy/src/error.rs`
- `crates/executor-policy/src/model.rs`
- `crates/executor-policy/src/decision.rs`
- `crates/executor-policy/src/selector.rs`
- `crates/executor-evm/src/normalize.rs`
- `crates/executor-evm/tests/normalize.rs`
- `.planning/phases/05-simulation-and-policy-gate/05-01-SUMMARY.md` (this file)

**Modified (9):**
- `Cargo.toml` (workspace members += executor-policy)
- `Cargo.lock` (transitive deltas)
- `crates/executor-evm/src/lib.rs` (re-exports + pub mod normalize)
- `crates/executor-evm/src/dyn_abi.rs` (encode_call_input + 4 tests + JsonAbi/Bytes/JsonAbiExt imports)
- `crates/executor-evm/src/action.rs` (dry_run_abi_encode delegates; unused JsonAbi + decode_err removed)
- `crates/executor-evm/src/erc20.rs` (ERC20_WRITE_ABI + 4 tests)
- `crates/executor-mcp/src/validation.rs` (MAX_ACTIONS_PER_RUN + 1 test)
- `crates/executor-mcp/src/tools.rs` (cap check in validate_strategy_output)
- `crates/executor-mcp/tests/stdio_handshake.rs` (2 stdio tests)

## Commits

| Task | Hash      | Message |
|------|-----------|---------|
| 1    | `3b215d8` | feat(05-01): scaffold executor-policy crate (alloy-free) + ERC20_WRITE_ABI + extract encode_call_input shared encoder |
| 2    | `6b90f6a` | feat(05-01): Action -> NormalizedAction normalize layer (per-variant table per D-02; reuses encode_call_input + ERC20_WRITE_ABI) |
| 3    | `dd78e27` | feat(05-01): MAX_ACTIONS_PER_RUN=32 cap at validate_strategy_output + stdio regression (D-12 / BR-02 carry-forward) |

## Self-Check: PASSED
- All 9 created files exist on disk.
- All 3 task commits present in `git log` (3b215d8, 6b90f6a, dd78e27).
- 388 / 388 workspace tests passing; clippy strict clean; alloy isolation D-20 verified.
