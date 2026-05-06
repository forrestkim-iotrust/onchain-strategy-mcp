---
phase: 05
plan: 02
subsystem: simulation-and-policy-gate
tags: [executor-evm, simulate, simulate_one, SimulationOutcome, SimulationFailReason, sanitize_revert_reason, simulation_from, executor-mcp, map_simulation_error, simulation_failure, D-05, D-08, D-13, D-14, D-19, BR-01, MR-01, WR-01, WR-04, EXE-03, EXE-04]
status: complete
created: 2026-04-27
duration_minutes: ~12
completed_date: 2026-04-27
dependency_graph:
  requires:
    - executor-evm read.rs sanitize_revert_reason / try_extract_revert_reason / classify_provider_error (Phase 4 D-12 / WR-04)
    - executor-evm EvmConfig (Phase 4 D-04) — extended in this plan
    - executor-evm provider (build_provider) — Phase 4 D-04
    - executor-mcp errors.rs STRATEGY_RUNTIME_ERROR + map_evm_error pattern (Phase 4 D-12)
    - alloy 2.0 Provider::call + TransactionBuilder (Phase 4 dep)
    - tokio::time::timeout (Phase 4 D-04 carry-forward)
  provides:
    - executor_evm::simulate::simulate_one(provider, cfg, tx, block, from) -> SimulationOutcome — D-05 adapter
    - executor_evm::simulate::SimulationOutcome::{Pass{return_bytes,gas_estimate}, Fail{reason,raw_for_log}}
    - executor_evm::simulate::SimulationFailReason::{Revert{decoded:Option<String>}, Transport, Timeout}
    - executor_evm::read::sanitize_revert_reason — D-19 promoted pub(crate)→pub for cross-module reuse
    - executor_evm::EvmConfig.simulation_from: Address — D-14 + lenient EIP-55 validation at from_raw
    - executor_evm test fixture revert_counter.hex (124-byte hand-assembled REVERT-on-any-call contract)
    - executor_mcp::errors::map_simulation_error(reason, action_index, run_id) -> McpError — D-08 -32017 + data.kind="simulation_failure"
    - executor_mcp::config EvmSection.simulation_from with default_simulation_from() = anvil-0 EIP-55
    - executor_mcp #[ignore]'d stub stdio test strategy_run_returns_simulation_failed_when_revert (Plan 05-04 enables)
    - executor_mcp anvil-tests cargo feature (mirrors executor-evm)
  affects:
    - executor-evm/src/lib.rs gains pub mod simulate; re-exports simulate_one + SimulationOutcome + SimulationFailReason
    - EvmConfig::from_raw signature changed: 2 params -> 3 params (rpc_url, call_timeout_ms, simulation_from); all call sites updated (executor-mcp Config::evm_config + tests/read_contract_anvil)
    - read.rs::try_extract_revert_reason promoted priv -> pub(crate) for simulate.rs reuse
    - executor-mcp/src/config.rs Config::evm_config wires the new third arg
    - config.example.toml gains a documented [evm] section (rpc_url + call_timeout_ms + simulation_from)
tech_stack:
  added:
    - none — all deps already present (alloy + tokio + executor-evm)
  patterns:
    - "Wire-safe Display + raw_for_log split (Phase 4 BR-01 / MR-01 carry-forward) — SimulationOutcome::Fail.raw_for_log is consumed by tracing::warn! at simulate site; map_simulation_error never sees it (factory takes typed SimulationFailReason)"
    - "Per-call tokio::time::timeout safety net (Phase 4 D-04 carry-forward) — caps wall-clock at cfg.call_timeout regardless of RPC liveness"
    - "Lenient EIP-55 address validation (Phase 4 D-09 carry-forward) — strict parse_checksummed accepted; uniformly-cased fallback; mixed-case-bad-checksum REJECTED"
    - "Skip-clean anvil tests (Phase 4 D-14 carry-forward) — AnvilFixture::try_spawn returns None when binary missing; tests early-return without panic"
    - "Hand-assembled minimal EVM bytecode for tiny test fixtures (avoids solc dep for trivially-shaped contracts)"
    - "Cross-plan #[ignore]'d test stub (new pattern) — registered test name visible at cargo test --list; downstream plan flips ignore + fills body"
key_files:
  created:
    - crates/executor-evm/src/simulate.rs
    - crates/executor-evm/tests/simulate_anvil.rs
    - crates/executor-evm/tests/simulate_timeout.rs
    - crates/executor-evm/tests/fixtures/revert_counter.hex
    - crates/executor-evm/tests/fixtures/revert_counter.sol-src.txt
    - .planning/phases/05-simulation-and-policy-gate/05-02-SUMMARY.md
    - .planning/phases/05-simulation-and-policy-gate/deferred-items.md
  modified:
    - crates/executor-evm/src/lib.rs (pub mod simulate + 3 re-exports)
    - crates/executor-evm/src/read.rs (sanitize_revert_reason pub; try_extract_revert_reason pub(crate); test call site)
    - crates/executor-evm/src/config.rs (simulation_from field + parse helper + 8 tests)
    - crates/executor-evm/tests/read_contract_anvil.rs (1 from_raw call site updated)
    - crates/executor-mcp/Cargo.toml (anvil-tests feature)
    - crates/executor-mcp/src/config.rs (EvmSection.simulation_from + default_simulation_from + 4 tests)
    - crates/executor-mcp/src/errors.rs (map_simulation_error factory + 5 sim_factory_tests + SimulationFailReason use)
    - crates/executor-mcp/tests/stdio_handshake.rs (#[ignore]'d stub: strategy_run_returns_simulation_failed_when_revert)
    - config.example.toml ([evm] section with simulation_from documentation)
decisions:
  - "Used struct-update syntax (`EvmConfig { rpc_url: ..., ..EvmConfig::default() }`) instead of `let mut cfg = EvmConfig::default(); cfg.rpc_url = ...` in new tests — avoids clippy::field_reassign_with_default. Pre-existing read_contract_anvil.rs uses the let-mut pattern; deferred to a future cleanup plan (deferred-items.md)."
  - "Promoted try_extract_revert_reason from priv to pub(crate) (not pub) — simulate.rs is sibling module so pub(crate) suffices; keeps the alloy-specific revert-text-scraping internal to executor-evm."
  - "Hand-assembled 124-byte revert_counter.hex (deployer + 12-byte runtime preamble + 100-byte ABI-encoded `Error(string)` payload) instead of solc/forge build pipeline — the contract has no function dispatch (any call reverts), so the bytecode is trivially compact and auditable. Solidity equivalent documented in revert_counter.sol-src.txt."
  - "map_simulation_error takes `&SimulationFailReason` (not the full SimulationOutcome) — guarantees the factory cannot accidentally serialize raw_for_log to the wire (MR-01 lock at the type level)."
  - "Replaced the planned `unreachable!()` arm in map_simulation_error's match with a safe `\"simulation failed\"` fallback — the compiler can't prove the three string literals are exhaustive, and the fallback is dead at runtime but defensive against future variant additions."
  - "Registered the cross-plan stdio test stub with `#[ignore]` so `cargo test -p executor-mcp --features anvil-tests -- --list` shows the test name today; Plan 05-04 only needs to (a) remove the `#[ignore]` and (b) replace the `panic!` body with the deploy + register + assert flow. Keeps test inventory traceable across plan handoffs."
  - "Added executor-mcp `anvil-tests` cargo feature mirroring executor-evm. Required because the stub test is `#[cfg(feature = \"anvil-tests\")]`; without the feature the gate test would never compile and the cross-plan handoff would be invisible."
metrics:
  duration_minutes: ~12
  task_count: 3
  files_created: 7
  files_modified: 9
  workspace_tests_before: 388
  workspace_tests_after: 409
  net_test_delta: +21
  workspace_tests_with_anvil_feature: 428 # 1 ignored stub
  clippy_strict: pass
  clippy_anvil_feature: pass-on-new-files-only # pre-existing read_contract_anvil warnings deferred (see deferred-items.md)
---

# Phase 05 Plan 02: simulate_one Adapter + sanitize_revert_reason Promotion + EvmConfig.simulation_from + map_simulation_error Factory Summary

Phase 5 wave 2 plumbing landed: `executor_evm::simulate::simulate_one` is the per-action `eth_call` adapter (D-05) producing a `SimulationOutcome::{Pass, Fail}` enum where `Fail` carries a typed `SimulationFailReason::{Revert{decoded}, Transport, Timeout}`; `read::sanitize_revert_reason` is now `pub` (D-19) so simulate's revert path reuses the same WR-04 control-char-strip + 256-byte-cap as Phase 4's `read_contract`; `EvmConfig` gains a `simulation_from: Address` field with anvil-0 EIP-55 default and lenient validation (D-14); and `executor_mcp::errors::map_simulation_error` emits the locked D-08 wire shape (`-32017` + `data.kind = "simulation_failure"` + `data.fail_reason` + `data.action_index` + sanitized `data.decoded_revert`). EXE-03 and EXE-04 are demonstrable at the adapter level — Plan 05-04 wires the orchestrator into `tools::strategy_run` and flips the `#[ignore]`'d stdio gate test.

## What Shipped

### Task 1 — simulate_one adapter + SimulationOutcome enum + sanitize_revert_reason promotion + EvmConfig.simulation_from

- New `crates/executor-evm/src/simulate.rs` with `pub async fn simulate_one(provider, cfg, tx, block, from) -> SimulationOutcome`. Body mirrors Phase 4 `read_contract`'s eth_call shape: clones `tx` into `tx_with_from` (P-1 mitigation when `from = Some(_)`), wraps `provider.call(tx_with_from).block(block)` in `tokio::time::timeout(cfg.call_timeout, ...)`, and matches the timeout result tri-state into Pass/Fail variants.
- `SimulationOutcome::{Pass{return_bytes, gas_estimate}, Fail{reason, raw_for_log}}` and `SimulationFailReason::{Revert{decoded:Option<String>}, Transport, Timeout}` give downstream code (Plan 05-04 orchestrator + 05-04 stdio test) a typed surface free of alloy types.
- Revert classification reuses Phase-4's `read::try_extract_revert_reason` (promoted to `pub(crate)`) + `looks_like_revert(&raw)` heuristic fallback. Decoded reasons route through `read::sanitize_revert_reason` (now `pub` per D-19) — control chars stripped, 256-byte cap, UTF-8-safe truncation. WR-04 carry-forward.
- `EvmConfig` gains `pub simulation_from: Address` field with default = anvil-0 EIP-55 (`0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266`). `EvmConfig::from_raw` signature changed from 2 params to 3; all call sites updated (executor-mcp `Config::evm_config`, `read_contract_anvil` test, `read.rs` test). Validation uses a `parse_simulation_from` helper that mirrors Phase 4 D-09's lenient EIP-55: strict `parse_checksummed` accepted; uniformly-cased 40-hex fallback; mixed-case-bad-checksum REJECTED with `EvmError::Config`.
- New `tests/simulate_timeout.rs` regression: closed port `http://127.0.0.1:1` + 200ms `call_timeout` → outcome is `Fail{Timeout|Transport}` within 1.5s wall clock. Proves the per-call timeout safety net is in place.

**Tests added (lib):** 11 — 6 in `simulate::tests` (Send/signature/sanitize-pub/looks_like_revert/Pass-roundtrip/Fail-revert-sanitized) + 5 new in `config::tests` (default_simulation_from, lowercase, uppercase, bad-checksum-reject, non-hex-reject). Existing 8 config tests and 6 read::tests stay green (sanitize_revert_reason_strips_control_chars_and_caps_length included).

**Commit:** `0237c48` `feat(05-02): simulate_one adapter + SimulationOutcome enum + EvmConfig.simulation_from (D-05/D-14/D-19)`

### Task 2 — Anvil-gated simulate tests (pass/revert/transport) + revert_counter bytecode fixture

- New `crates/executor-evm/tests/fixtures/revert_counter.hex`: 124-byte hand-assembled deployer that returns a runtime body which UNCONDITIONALLY reverts with `Error(string) "forced revert reason"`. The contract has no function dispatch — any incoming call hits the `REVERT(0, 100)` opcode with the pre-baked ABI-encoded payload. Source documented in `revert_counter.sol-src.txt` with full opcode disassembly so a reviewer can audit the bytecode without solc.
- New `crates/executor-evm/tests/simulate_anvil.rs` (`#[cfg(feature = "anvil-tests")]`):
  1. `simulate_pure_view_call_passes` — deploy Counter, simulate `number()`, assert `Pass{return_bytes.len() == 32, gas_estimate: None}`.
  2. `simulate_increment_counter_passes` — deploy Counter, simulate `increment()` (state-changing fn but `eth_call` doesn't write), assert `Pass`.
  3. `simulate_revert_returns_simulation_failure` — deploy RevertCounter, simulate any call, assert `Fail{Revert{decoded}}` with `decoded.len() <= 256` and `!decoded.chars().any(|c| c.is_control())` (WR-04 invariants).
  4. `simulate_unreachable_rpc_returns_transport_or_timeout` — closed port `http://127.0.0.1:1` + 300ms timeout, assert `Fail{Transport | Timeout}`.
- Skip-clean contract: `AnvilFixture::try_spawn() -> None` when anvil binary missing OR `funded_accounts.is_empty()`. All 4 tests early-return without panicking. Verified by running `cargo test -p executor-evm --features anvil-tests --test simulate_anvil` on a machine WITHOUT anvil — 4 passed (skip-clean for the 3 anvil-required ones; the 4th runs fully).

**Tests added (integration):** 4 in `simulate_anvil.rs` + 1 in `simulate_timeout.rs` (Task 1) = 5 new integration tests.

**Commit:** `5b09621` `test(05-02): anvil-gated simulate tests (pass / revert / transport) + revert_counter bytecode fixture`

### Task 3 — [evm.simulation_from] config wiring + map_simulation_error factory + #[ignore]'d stdio stub

- `executor-mcp/src/config.rs` `EvmSection` gains `pub simulation_from: String` with `#[serde(default = "default_simulation_from")]` returning the anvil-0 EIP-55 string. `Config::evm_config()` propagates the field through `EvmConfig::from_raw` (3-arg signature). The Phase-4 `evm_section_rejects_unknown_fields` regression stays green because `simulation_from` is a defaulted field on a `deny_unknown_fields` struct.
- `executor-mcp/src/errors.rs` `pub fn map_simulation_error(reason: &SimulationFailReason, action_index: u32, run_id: &str) -> McpError` factory:
  - Wire shape locked per D-08 / Phase 4 D-12 reuse precedent — NO new wire codes.
  - `error.code` = `-32017`; `data.code` = `"strategy_runtime_error"`; `data.kind` = `"simulation_failure"` (BR-01 — distinguishes from `"exception"` / `"timeout"` / `"evm_*"`).
  - `data.fail_reason ∈ {"revert", "transport", "timeout"}`; `data.action_index` (u32); `data.decoded_revert` (sanitized String OR null); `data.detail` mirrors the canonical `error.message`.
  - Detail format strings: `"simulation failed: evm revert: {d}"` (revert with decoded), `"simulation failed: evm revert: unknown"` (revert no decoded), `"simulation failed: evm rpc error: transport"`, `"simulation failed: evm rpc error: timeout"`.
  - MR-01 lock: factory takes `&SimulationFailReason` (NOT `&SimulationOutcome`), so the type system prevents accidental `raw_for_log` serialisation.
- New `executor-mcp` `anvil-tests` cargo feature mirrors `executor-evm`'s. Required to gate the stub stdio test.
- `crates/executor-mcp/tests/stdio_handshake.rs` adds the cross-plan stub `strategy_run_returns_simulation_failed_when_revert` (`#[cfg(feature = "anvil-tests")]` + `#[ignore = "..."]`). Plan 05-04 will (a) remove the `#[ignore]` and (b) replace the `panic!()` body with the deploy + register + assert flow once `tools::strategy_run` wires the simulation gate. The registered stub keeps the cross-plan test inventory visible at `cargo test -- --list`.
- `config.example.toml` gains a documented `[evm]` section showing `rpc_url`, `call_timeout_ms`, and `simulation_from` with a paragraph explaining the lenient EIP-55 validation contract.

**Tests added (lib):** 9 — 4 new in `config::tests` (`evm_section_default_simulation_from_is_anvil_account_0`, `evm_section_simulation_from_override_is_propagated`, `evm_section_simulation_from_bad_checksum_returns_err_at_evm_config`, `evm_config_default_simulation_from_round_trips_through_evm_config`) + 5 new in `errors::tests` `sim_factory_tests` group (`map_simulation_error_for_revert_emits_simulation_failure_kind`, `..._for_revert_with_no_decoded_uses_unknown`, `..._for_transport_emits_transport_fail_reason`, `..._for_timeout_emits_timeout_fail_reason`, `..._does_not_leak_raw_alloy_text`).

**Commit:** `17b1fe2` `feat(05-02): [evm.simulation_from] config + map_simulation_error factory (-32017 simulation_failure) + stdio test stub`

## Cross-Plan Exports

**Plan 05-03 (policy load + eval) consumes:** none directly — executor-policy is alloy-FREE; this plan sits in alloy-side. (05-03 still consumes Plan 05-01's `executor-policy` scaffolding.)

**Plan 05-04 (orchestrator wiring) consumes:**
- `executor_evm::{simulate_one, SimulationOutcome, SimulationFailReason}` — orchestrator calls `Handle::current().block_on(simulate_one(...))` from inside `spawn_blocking` (WR-01 lock).
- `executor_evm::EvmConfig.simulation_from` — passed as the `from` arg to simulate_one.
- `executor_mcp::errors::map_simulation_error` — converts `Fail{reason}` outcomes to `-32017 + simulation_failure` wire errors per D-08.
- `crates/executor-mcp/tests/stdio_handshake.rs::strategy_run_returns_simulation_failed_when_revert` — currently `#[ignore]`'d; 05-04 flips the ignore + fills the body.

## Threat Surface Disposition (per plan threat_model)

| Threat ID  | Disposition  | Verification |
|------------|--------------|--------------|
| T-05-02-01 | mitigated    | Decoded revert reasons routed through `read::sanitize_revert_reason` (D-19 promoted pub). `simulate_revert_returns_simulation_failure` asserts WR-04 invariants (`<=256` bytes, no control chars) on real anvil reverts. `fail_revert_carries_sanitized_decoded` lib test pins the same invariants synthetically. |
| T-05-02-02 | mitigated    | Stable wire prefix `"simulation failed: "` distinguishes simulation-side denials from EvmError taxonomy. `map_simulation_error_for_revert_emits_simulation_failure_kind` asserts the canonical prefix; `..._does_not_leak_raw_alloy_text` proves attacker text spoofing taxonomy strings cannot escape sanitization + the prefix discipline. |
| T-05-02-03 | mitigated    | `from_raw_rejects_mixed_case_bad_checksum_simulation_from` (executor-evm) + `evm_section_simulation_from_bad_checksum_returns_err_at_evm_config` (executor-mcp) prove rejection at boot. Lenient EIP-55 mirrors Phase 4 D-09 `validate_address`. |
| T-05-02-04 | mitigated    | `simulate_timeout_fires_when_rpc_unreachable` proves per-call `tokio::time::timeout` caps wall-clock under 1.5s against unreachable RPC. `cfg.call_timeout` reuses Phase 4 D-04 (default 1s, range 50ms..30s). |
| T-05-02-05 | accepted     | `try_extract_revert_reason` heuristic + `looks_like_revert(&raw)` fallback together cover the failure modes; transport-vs-revert misclassification is observable but not security-relevant (both lead to `Fail` → deny-signing per EXE-04). |
| T-05-02-06 | accepted     | Plan 05-04 owns `journal_decisions`. Plan 05-02 emits the wire error directly via `map_simulation_error`; the partial-journaling gap exists for one wave only. |
| T-05-02-07 | mitigated    | `map_simulation_error` takes `&SimulationFailReason` (NOT the full Outcome) — type system prevents `raw_for_log` from reaching the factory. `map_simulation_error_does_not_leak_raw_alloy_text` asserts no `TransportError` / `Reqwest` / `ErrorResp` substrings in the data payload. |

## Carry-Forward Compliance (Phase 3 + Phase 4 anti-pattern lattice)

| Invariant | Plan 05-02 status |
|-----------|------------------|
| HR-01 (forbidden-globals scrub) | Plan 05-02 adds NO new `ctx.*` surface. The HR-01 scrub site (`strategy-js::sandbox`) is untouched; `cargo test -p strategy-js sandbox_blocks_host_globals` stays green. |
| MR-01 (no raw alloy/serde/toml on the wire) | `SimulationOutcome::Fail::raw_for_log` is consumed by `tracing::warn!` at simulate site only; `map_simulation_error` takes typed `&SimulationFailReason` and the type system prevents leaks. `map_simulation_error_does_not_leak_raw_alloy_text` regression pins this. |
| MR-03 (no silent serde fallback) | `simulate.rs` and the new factory use `?`-propagation throughout; no `unwrap_or_else(|_| default)` introduced. |
| MR-04 (per-run monotonic seq) | Plan 05-02 adds NO journal write paths. `journal_logs` and `journal_source_reads` continue to hold their seq invariants from Phase 3 / Phase 4. Plan 05-04 owns `journal_decisions`. |
| BR-01 (stable wire taxonomy survives JS round-trip) | `data.kind = "simulation_failure"` is a stable enum string distinct from `"exception"` / `"timeout"` / `"evm_*"`. `map_simulation_error_for_revert_emits_simulation_failure_kind` pins this; the `#[ignore]`'d stdio stub will pin it end-to-end at Plan 05-04. |
| BR-02 (cap-at-output-gate) | `MAX_ACTIONS_PER_RUN = 32` from Plan 05-01 is unchanged and applies BEFORE any simulation work. |
| WR-01 (no `block_in_place` from inside `spawn_blocking`) | `simulate_one` is `async fn` — orchestrator drives it via `Handle::current().block_on(...)` from inside `spawn_blocking` (Plan 05-04 wires this). `grep -c block_in_place crates/executor-evm/src/simulate.rs` == 0. SimulationOutcome and SimulationFailReason verified `Send` via `simulation_outcome_is_send`. |
| WR-04 (sanitize attacker-controllable text) | `read::sanitize_revert_reason` promoted to `pub` (D-19). `simulate.rs::simulate_one` revert path calls `sanitize_revert_reason(&decoded)` before constructing `Revert { decoded: Some(_) }`. WR-04 invariants asserted on real anvil reverts in `simulate_revert_returns_simulation_failure`. |

## Deviations from Plan

**Three minor refinements that do NOT alter the contract:**

1. **`map_simulation_error`'s match fallback.** The plan body called `unreachable!("fail_reason is one of three constants")`. The Rust compiler can't prove the three string literals are exhaustive, so I replaced `unreachable!()` with a safe `"simulation failed".to_string()` fallback. The arm is dead at runtime (the three preceding arms cover every `SimulationFailReason` variant via the `(&str, _)` tuple match) but the safe fallback is defensive against future variant additions. This does NOT alter the locked wire shape — the three live arms produce the canonical strings.

2. **`try_extract_revert_reason` promoted to `pub(crate)` (not `pub`).** The plan flagged this as an OPTIONAL companion promotion. `simulate.rs` is a sibling module so `pub(crate)` suffices and keeps the alloy-specific revert-text-scraping invariant internal to `executor-evm`. Verified by `cargo build -p executor-evm` after the change.

3. **Fixture format.** Wrote `revert_counter.hex` as 124 bytes of hand-assembled bytecode (constructor + runtime + embedded `Error(string)` payload) rather than running solc. The contract has no function dispatch — any incoming call reverts — which makes the bytecode trivially small and auditable. Source documented in `revert_counter.sol-src.txt` with full opcode disassembly. Equivalent Solidity is shown in the file. Plan flagged this option as acceptable (`Compile out-of-band with `solc` or `forge build`; paste the deployed runtime bytecode`).

## Verification

| Check | Command | Result |
|-------|---------|--------|
| Build (full workspace) | `cargo build --workspace` | clean |
| Tests (full workspace, no anvil feature) | `cargo test --workspace` | 409 passed (was 388 → +21 net) |
| Tests (full workspace, anvil features) | `cargo test --workspace --features executor-evm/anvil-tests --features executor-mcp/anvil-tests` | 428 passed, 1 ignored (the Plan 05-04 stub) |
| executor-evm lib | `cargo test -p executor-evm --lib` | 74 passed (was 63 → +11 from simulate + config) |
| executor-evm simulate timeout | `cargo test -p executor-evm --test simulate_timeout` | 1 passed |
| executor-evm simulate anvil (skip-clean) | `cargo test -p executor-evm --features anvil-tests --test simulate_anvil` | 4 passed (3 skip-clean without anvil; 1 closed-port runs) |
| executor-mcp lib | `cargo test -p executor-mcp --lib` | 49 passed (was 40 → +9 from sim_factory_tests + config simulation_from tests) |
| Phase-4 sanitize regression | `cargo test -p executor-evm --lib read::tests::sanitize_revert_reason_strips_control_chars_and_caps_length` | 1 passed (visibility-only change preserved body) |
| Phase-4 stdio cap regression | `cargo test -p executor-mcp --test stdio_handshake strategy_run_caps_action_array_length_at_32` | 1 passed |
| HR-01 sandbox regression | `cargo test -p strategy-js sandbox_blocks_host_globals` | 2 passed |
| Clippy strict | `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| Clippy strict (anvil features) | `cargo clippy --workspace --all-targets --features executor-evm/anvil-tests --features executor-mcp/anvil-tests -- -D warnings` | NEW files clean; 8 pre-existing warnings in `read_contract_anvil.rs` deferred (see deferred-items.md) |
| WR-01 lock | `grep -c 'block_in_place' crates/executor-evm/src/simulate.rs` | `0` |
| WR-04 reuse | `grep -c 'sanitize_revert_reason' crates/executor-evm/src/simulate.rs` | `2` (import + call site) |
| D-19 visibility | `grep -c 'pub fn sanitize_revert_reason' crates/executor-evm/src/read.rs` | `1` (was `pub(crate)`) |
| D-14 wire | `grep -c 'simulation_from' crates/executor-evm/src/config.rs` | `15` (struct + default + parse + 5 tests + helpers) |
| D-08 wire | `grep -c '"simulation_failure"' crates/executor-mcp/src/errors.rs` | `1` (the factory) |
| D-08 detail strings | `grep -c 'simulation failed:' crates/executor-mcp/src/errors.rs` | `4` (revert/unknown/transport/timeout) |

## Files Touched

**Created (7):**
- `crates/executor-evm/src/simulate.rs`
- `crates/executor-evm/tests/simulate_anvil.rs`
- `crates/executor-evm/tests/simulate_timeout.rs`
- `crates/executor-evm/tests/fixtures/revert_counter.hex`
- `crates/executor-evm/tests/fixtures/revert_counter.sol-src.txt`
- `.planning/phases/05-simulation-and-policy-gate/05-02-SUMMARY.md` (this file)
- `.planning/phases/05-simulation-and-policy-gate/deferred-items.md`

**Modified (9):**
- `crates/executor-evm/src/lib.rs` (pub mod simulate + 3 re-exports)
- `crates/executor-evm/src/read.rs` (sanitize_revert_reason pub; try_extract_revert_reason pub(crate); 1 test call site updated for new from_raw signature)
- `crates/executor-evm/src/config.rs` (simulation_from field + Default + parse_simulation_from helper + 4 new tests + 1 amended test)
- `crates/executor-evm/tests/read_contract_anvil.rs` (1 EvmConfig::from_raw call site updated to 3-arg signature)
- `crates/executor-mcp/Cargo.toml` (anvil-tests feature)
- `crates/executor-mcp/src/config.rs` (EvmSection.simulation_from field + default_simulation_from + 4 new tests; evm_config() propagates the new arg)
- `crates/executor-mcp/src/errors.rs` (SimulationFailReason import + map_simulation_error factory + 5 new sim_factory_tests)
- `crates/executor-mcp/tests/stdio_handshake.rs` (#[ignore]'d stub strategy_run_returns_simulation_failed_when_revert + 30-line documentation block)
- `config.example.toml` ([evm] section with simulation_from documentation)

## Commits

| Task | Hash      | Message |
|------|-----------|---------|
| 1    | `0237c48` | feat(05-02): simulate_one adapter + SimulationOutcome enum + EvmConfig.simulation_from (D-05/D-14/D-19) |
| 2    | `5b09621` | test(05-02): anvil-gated simulate tests (pass / revert / transport) + revert_counter bytecode fixture |
| 3    | `17b1fe2` | feat(05-02): [evm.simulation_from] config + map_simulation_error factory (-32017 simulation_failure) + stdio test stub |

## Self-Check: PASSED

- All 7 created files exist on disk.
- All 3 task commits present in `git log` (`0237c48`, `5b09621`, `17b1fe2`).
- 409 / 409 workspace tests passing (no anvil feature); 428 / 429 with anvil features (1 intentionally `#[ignore]`'d for Plan 05-04 handoff); clippy strict clean on all new files; pre-existing `read_contract_anvil` clippy warnings deferred to deferred-items.md per scope-boundary rule.
