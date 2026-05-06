---
phase: 04-evm-context-and-actions
verified: 2026-04-27T00:00:00Z
status: passed
score: 9/9 must-haves verified
verdict: PASS
re_verification:
  previous_status: none
  previous_score: n/a
  gaps_closed: []
  gaps_remaining: []
  regressions: []
overrides_applied: 0
---

# Phase 4: EVM Context and Actions — Verification Report

**Phase Goal:** Strategy code can express broad EVM reads and write actions through `ctx`.
**Verified:** 2026-04-27
**Status:** PASS
**Re-verification:** No — initial verification

---

## Goal Achievement

### Observable Truths (from ROADMAP success criteria + CTX-01..09)

| #   | Truth                                                                              | Status     | Evidence                                                                                                                                                                |
| --- | ---------------------------------------------------------------------------------- | ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `ctx.evm.readContract` reads arbitrary ABI-compatible contract methods (CTX-01)    | ✓ VERIFIED | `crates/executor-evm/src/read.rs` (14.3K, full eth_call lifecycle); `ctx.evm.readContract` host binding present in `crates/strategy-js/src/sandbox.rs`; 6 anvil-gated read_contract_anvil tests pass under `--features anvil-tests` |
| 2   | ERC20/native read helpers work against a local EVM (CTX-02..04)                    | ✓ VERIFIED | `executor-evm/src/erc20.rs` (6 helpers) + `native.rs`; `ctx.evm.readErc20.*` + `readNative.*` + flat aliases (`erc20Balance`, `erc20Allowance`, `nativeBalance`); 12 ctx_evm_helpers tests pass; 9 anvil-gated erc20/native tests pass with anvil installed |
| 3   | `contractCall`, `rawCall`, ERC20, and native actions produce validated `Action[]` (CTX-05..08) | ✓ VERIFIED | `Action` enum has 5 new variants in `executor-core/src/schema/action.rs` (47 grep matches); `phase4_emittable()` gate present; validator allowlist exact 6 kinds (validation.rs:81-86 + 217-222); `ctx.actions.*` builders wired in sandbox.rs; 5 `strategy_run_accepts_*` stdio tests at lines 1282/1313/1335/1357/1379 |
| 4   | Unit/address helpers reduce common amount/address errors (CTX-09)                  | ✓ VERIFIED | `executor-evm/src/units.rs` (11.6K) + `address.rs` (6.9K); `ctx.units.{parseUnits,formatUnits}` + `ctx.address.{isAddress,checksum,zeroAddress}` in sandbox; 22 ctx_units_address tests + 14 units lib tests pass |
| 5   | CTX-01 (readContract ABI generic reads)                                            | ✓ VERIFIED | covered by Truth #1                                                                                                                                                    |
| 6   | CTX-02..04 (erc20Balance, erc20Allowance, nativeBalance read helpers)              | ✓ VERIFIED | covered by Truth #2                                                                                                                                                    |
| 7   | CTX-05..08 (contractCall, rawCall, erc20Approve/Transfer, nativeTransfer actions)  | ✓ VERIFIED | covered by Truth #3                                                                                                                                                    |
| 8   | CTX-09 (units + address helpers)                                                   | ✓ VERIFIED | covered by Truth #4                                                                                                                                                    |
| 9   | Phase-3 carry-forward (HR-01, MR-01, MR-03, MR-04) preserved                       | ✓ VERIFIED | `sandbox_blocks_host_globals` 8/8 green; `seq INTEGER NOT NULL + UNIQUE (run_id, seq)` on `journal_source_reads` (schema.rs:46-47, 74-75); `EvmError::Display` taxonomy strings only on wire; all 349 workspace tests pass |

**Score:** 9/9 truths verified

---

## Required Artifacts

| Artifact                                                          | Expected                                | Status     | Details                                                       |
| ----------------------------------------------------------------- | --------------------------------------- | ---------- | ------------------------------------------------------------- |
| `crates/executor-evm/Cargo.toml`                                  | new crate manifest, alloy 2.0.x         | ✓ VERIFIED | `cargo tree -p executor-evm` reports `alloy v2.0.1`          |
| `crates/executor-evm/src/{lib,error,config,provider,dyn_abi,read,erc20,native,action,units,address}.rs` | full module tree | ✓ VERIFIED | All 11 source files present (verified via `ls`)              |
| `crates/executor-evm/tests/common/anvil_fixture.rs` + fixtures    | anvil fixture + counter.hex + erc20.hex | ✓ VERIFIED | Files present; `try_spawn` skips cleanly when anvil missing  |
| `crates/executor-core/src/schema/action.rs` extended              | 5 new variants + phase4_emittable + 64 KiB cap | ✓ VERIFIED | 47 grep matches across ContractCall/RawCall/Erc20*/NativeTransfer; `MAX_ABI_BYTES` referenced from action.rs:26 |
| `crates/executor-mcp/src/validation.rs` widened                   | exact 6-kind allowlist                  | ✓ VERIFIED | Lines 81-86 and 217-222: noop, contract_call, raw_call, erc20_transfer, erc20_approve, native_transfer |
| `crates/executor-mcp/src/errors.rs` extended                      | `data.kind ∈ {evm_rpc_error, evm_decode_error, evm_revert}` | ✓ VERIFIED | `executor-evm/src/error.rs:52` `data_kind()` dispatches into the three new kinds; alongside Phase-3 set |
| `crates/executor-state/src/schema.rs` `seq` column                | `journal_source_reads.seq INTEGER NOT NULL + UNIQUE (run_id, seq)` | ✓ VERIFIED | schema.rs:46-47 (journal_logs) + 74-75 (journal_source_reads) — both tables carry the constraint |
| Schema goldens: Action.json + 5 per-variant                       | 6 new/regenerated files                 | ✓ VERIFIED | All 6 files present in `crates/executor-core/tests/schemas/` |
| Strategy-js sandbox additive bindings                              | ctx.evm / ctx.units / ctx.address / extended ctx.actions | ✓ VERIFIED | `crates/strategy-js/src/sandbox.rs` — all 4 sub-objects installed BEFORE FORBIDDEN_GLOBALS_SCRUB |
| stdio handshake: `strategy_run_accepts_*` (5)                     | per-variant accept tests (D-16 flip)    | ✓ VERIFIED | 5 tests in `crates/executor-mcp/tests/stdio_handshake.rs` lines 1282/1313/1335/1357/1379 |
| Old reject test (`strategy_run_rejects_phase4_action_kind`) gone  | D-16 flip                                | ✓ VERIFIED | grep returns 0 matches in stdio_handshake.rs                  |

---

## Key Link Verification

| From                          | To                                                  | Via                                                                  | Status   |
| ----------------------------- | --------------------------------------------------- | -------------------------------------------------------------------- | -------- |
| strategy-js sandbox           | executor-evm                                        | path-dep + re-export of `DynProvider`                                | ✓ WIRED  |
| strategy-js sandbox           | host-side dispatch (block_on inside spawn_blocking) | tokio Handle::try_current + block_in_place                           | ✓ WIRED  |
| executor-mcp tools.rs         | executor-evm provider via ExecutorServer            | `OnceCell<Arc<DynProvider>>` lazy-init + `with_evm` chain            | ✓ WIRED  |
| ctx.evm.* call                | journal_source_reads (kind="evm_read")              | RuntimeContext::flush drains evm_reads with MR-03 ?-propagation      | ✓ WIRED  |
| Action enum 6 variants        | StrategyOutcome.json / StrategyRunResponse.json     | schemars regeneration (04-03)                                        | ✓ WIRED  |
| EvmError variants             | -32017 with stable taxonomy                         | `map_evm_error` + `RuntimeError::Evm(EvmError)` `#[from]`            | ✓ WIRED  |
| strategy-js (no alloy)        | D-02 isolation                                      | `cargo tree -p strategy-js | grep '^alloy'` returns nothing          | ✓ WIRED  |

---

## Decision Verification (D-01..D-16)

| D-#  | Decision                                                                                   | Status   | Evidence                                                                                                |
| ---- | ------------------------------------------------------------------------------------------ | -------- | ------------------------------------------------------------------------------------------------------- |
| D-01 | alloy 2.0.x pinned to executor-evm only                                                    | ✓        | `cargo tree -p executor-evm` shows `alloy v2.0.1`; not in workspace.dependencies                       |
| D-02 | New executor-evm crate; strategy-js stays alloy-free                                       | ✓        | `cargo tree -p strategy-js | grep '^alloy'` returns nothing                                            |
| D-03 | Decimal-string BigInt bridge; BigInt rejected with stable message                          | ✓        | 11 `dyn_abi_roundtrip` tests pass; ctx_actions_negative_grid covers BigInt rejection                   |
| D-04 | Lazy `Arc<DynProvider>` per ExecutorServer; per-call timeout                               | ✓        | Server constructor uses `OnceCell<Arc<DynProvider>>`; lazy-init confirmed in 04-01 SUMMARY             |
| D-05 | readContract input shape `{address, abi, function, args, blockTag?}`; abi as string OR JS array | ✓     | covered by ctx_evm_read_contract tests                                                                  |
| D-06 | ERC20 helpers: balanceOf, allowance, decimals, symbol, name, totalSupply                   | ✓        | 6 helpers in erc20.rs + bundled OZ ABI; flat aliases for CTX-02/03                                     |
| D-07 | Native: balance, blockNumber; chainId omitted                                              | ✓        | `native.rs` exposes both; no chainId surface                                                            |
| D-08 | Action enum + 5 new variants + phase4_emittable + 64 KiB ABI cap                           | ✓        | All 5 variants present (47 matches); `MAX_ABI_BYTES = 65_536` in executor-core schema                  |
| D-09 | Validator allowlist exact 6 kinds                                                          | ✓        | validation.rs:81-86, 217-222 — noop + 5 phase-4 kinds                                                  |
| D-12 | -32017 with `data.kind ∈ {evm_rpc_error, evm_decode_error, evm_revert}`                    | ✓        | `EvmError::data_kind()` (error.rs:52) dispatches all three; raw text NEVER on wire                      |
| D-13 | ctx.evm.* journals to journal_source_reads with seq, kind="evm_read"                        | ✓        | `record_evm_read` flush drains into table; payload.helper records structured-form name                  |
| D-14 | Anvil tests behind `--features anvil-tests`; clean skip without binary                     | ✓        | `cargo test --workspace --features anvil-tests` → 364 passed; default 349 (anvil tests skip cleanly)   |
| D-15 | HR-01/MR-01/MR-03/MR-04 carry-forward preserved                                            | ✓        | `sandbox_blocks_host_globals` 8/8; UNIQUE (run_id, seq) on journal_source_reads; stable wire taxonomy   |
| D-16 | Phase-3 reject-test FLIPPED                                                                | ✓        | 0 matches for old name; 5 `strategy_run_accepts_*` tests at lines 1282/1313/1335/1357/1379             |

---

## Behavioral Spot-Checks

| Behavior                                                          | Command                                                                            | Result        | Status |
| ----------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ------------- | ------ |
| Workspace test suite passes                                       | `cargo test --workspace`                                                           | 349 passed    | ✓ PASS |
| Workspace test suite passes WITH anvil feature                    | `cargo test --workspace --features anvil-tests`                                    | 364 passed    | ✓ PASS |
| Clippy clean (-D warnings)                                        | `cargo clippy --workspace --all-targets -- -D warnings`                            | no warnings   | ✓ PASS |
| alloy 2.0.x pinned to executor-evm                                | `cargo tree -p executor-evm | head`                                                | `alloy v2.0.1`| ✓ PASS |
| strategy-js has no direct alloy dep                               | `cargo tree -p strategy-js | grep '^alloy'`                                        | empty         | ✓ PASS |
| D-16 flip — old test name absent                                  | `grep -c strategy_run_rejects_phase4_action_kind crates/.../stdio_handshake.rs`    | 0             | ✓ PASS |
| 5 per-variant accept tests present                                | `grep -c 'async fn strategy_run_accepts_' crates/.../stdio_handshake.rs`           | 5             | ✓ PASS |

---

## Requirements Coverage (CTX-01..09)

| Requirement | Description                                                       | Source Plan | Status      | Evidence                                                                       |
| ----------- | ----------------------------------------------------------------- | ----------- | ----------- | ------------------------------------------------------------------------------ |
| CTX-01      | `ctx.evm.readContract` ABI-based generic reads                    | 04-01       | ✓ SATISFIED | read_contract_anvil tests + ctx_evm_read_contract sandbox tests                |
| CTX-02      | `ctx.evm.erc20Balance` reads ERC20 balances                       | 04-02       | ✓ SATISFIED | erc20_helpers_anvil + flat alias tests in ctx_evm_helpers                      |
| CTX-03      | `ctx.evm.erc20Allowance` reads ERC20 allowances                   | 04-02       | ✓ SATISFIED | erc20_helpers_anvil + ctx_evm_helpers flat-alias coverage                      |
| CTX-04      | `ctx.evm.nativeBalance` reads native balance                      | 04-02       | ✓ SATISFIED | native_helpers_anvil + ctx_evm_helpers flat-alias coverage                     |
| CTX-05      | `ctx.actions.contractCall` ABI-based contract calls               | 04-03       | ✓ SATISFIED | strategy_run_accepts_contract_call (stdio_handshake.rs:1282)                   |
| CTX-06      | `ctx.actions.rawCall` raw calldata                                | 04-03       | ✓ SATISFIED | strategy_run_accepts_raw_call (stdio_handshake.rs:1313)                        |
| CTX-07      | `ctx.actions.erc20Approve` + `ctx.actions.erc20Transfer`          | 04-03       | ✓ SATISFIED | accepts_erc20_transfer + accepts_erc20_approve (lines 1335 + 1357)             |
| CTX-08      | `ctx.actions.nativeTransfer`                                      | 04-03       | ✓ SATISFIED | strategy_run_accepts_native_transfer (line 1379)                               |
| CTX-09      | `ctx.units` + address helpers                                     | 04-04       | ✓ SATISFIED | ctx_units_address (22 tests) + units lib (14) + address lib                    |

**Coverage:** 9/9 requirements satisfied. No orphaned requirements.

---

## Anti-Pattern Scan

| File                            | Pattern                              | Severity | Impact                                              |
| ------------------------------- | ------------------------------------ | -------- | --------------------------------------------------- |
| (none found in Phase-4 surface) | n/a                                  | n/a      | Clippy clean; no TODO/FIXME/placeholder in new code |

D-15 carry-forward verified:
- **HR-01:** FORBIDDEN_GLOBALS_SCRUB still runs BEFORE host bindings. New ctx.evm/ctx.units/ctx.address sub-objects build at the SAME injection site as ctx.actions.
- **MR-01:** EvmError::Display only emits stable taxonomy strings (`evm rpc error: transport`, `evm decode error: <category>`, `evm revert: <reason>`, `evm rpc error: timeout`); raw alloy/reqwest text routed via `tracing::warn!` only.
- **MR-03:** `record_evm_read` flush ?-propagates serde failures via `StateError::SerializationError`.
- **MR-04:** `journal_source_reads.seq INTEGER NOT NULL + UNIQUE (run_id, seq)` schema-level guarantee + 3 regression tests (`journal_source_read_seq.rs`).

---

## NOTE-1..NOTE-4 from plan-checker — folded?

| Note | Topic                                      | Status                                                                                                       |
| ---- | ------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| NOTE-1 | zeroAddress reassignment doesn't corrupt host view | ✓ folded — pinned by `zero_address_local_reassignment_does_not_corrupt_host_view` test (04-04 SUMMARY)        |
| NOTE-2 | Default blockTag = Latest when missing or undefined | ✓ folded — pinned by `flat_alias_default_blockTag_is_latest` (04-02 SUMMARY)                                  |
| NOTE-3 | Revert vs Decode classification           | ✓ folded — documented as known limitation in 04-01 SUMMARY; integration test accepts `kind ∈ {evm_decode_error, evm_revert}` |
| NOTE-4 | clock source for journal records          | ✓ folded — host.now_millis()/chrono in executor-state, no chrono leak into strategy-js (04-01 SUMMARY)        |

---

## Phase 5 Unblock Check

| Prerequisite                                | Status    |
| ------------------------------------------- | --------- |
| StateStore stable                           | ✓ stable  |
| Sandbox stable                              | ✓ stable  |
| RuntimeContext extended (with_evm)          | ✓ stable  |
| strategy_run wired with EVM provider        | ✓ stable  |
| executor-evm crate boundary clean           | ✓ stable  |
| Action[] 6-variant wire shape locked        | ✓ stable (goldens committed) |

Phase 5 is unblocked.

---

## Gaps Summary

**None.** All 9 must-haves verified, all 14 decisions (D-01..D-16, minus D-10/D-11 which are sub-aspects of D-09's per-kind validation captured under Truth #4) honoured in code, all 4 plan SUMMARY claims line up with the codebase.

`cargo test --workspace` passes 349/349; with `--features anvil-tests` the suite passes 364/364 (anvil binary present locally — anvil-gated tests fully exercised). No blockers, no warnings, no human verification needed.

---

## Verdict: PASS

- Phase goal achieved: strategy code can express broad EVM reads and write actions through `ctx`.
- All ROADMAP success criteria (1-4) verified end-to-end.
- All CTX-01..CTX-09 requirements satisfied with automated test evidence.
- All locked decisions D-01..D-16 reflected in code.
- D-15 anti-pattern carry-forward preserved (HR-01, MR-01, MR-03, MR-04).
- D-16 reject-test flip executed cleanly (0 grep matches for old name; 5 per-variant accept tests landed).
- 349 workspace tests pass on default; 364 pass with `--features anvil-tests`; clippy clean.

---

_Verified: 2026-04-27_
_Verifier: Claude (gsd-verifier)_
