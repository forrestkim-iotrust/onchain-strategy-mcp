---
phase: 05
plan: 03
subsystem: simulation-and-policy-gate
tags: [executor-policy, load_policy_from_path, evaluate, LoadedPolicy, SelectorPattern, ChainContract, Erc20SpendCap, RawCallAllowResolved, [policy]-section, ExecutorServer.policy, map_policy_error, policy_not_loaded, policy_get, POL-01, POL-02, POL-03, POL-04, POL-05, POL-06, D-06, D-08, D-13, D-15, D-16, D-20, MR-01, MR-03, BR-02]
status: complete
created: 2026-04-27
duration_minutes: ~30
completed_date: 2026-04-27
dependency_graph:
  requires:
    - executor-policy crate scaffold (Plan 05-01) — PolicyConfig, PolicyError, Decision, DecisionVerdict, NormalizedActionKindCopy
    - alloy-primitives 1.x for Address / U256 (D-20 — alloy umbrella crate forbidden)
    - executor-mcp Phase 4 [evm] section pattern (mirror for [policy])
    - executor-mcp errors.rs map_evm_error / map_simulation_error templates (Phase 4 D-12 / Plan 05-02 D-08)
    - executor-mcp ExecutorServer Phase 4 evm_provider OnceCell pattern (mirror for policy field)
    - tempfile dev-dep on executor-mcp (Phase 2 carry-forward) for state_path tmpdir in boot tests
  provides:
    - executor_policy::load::{load_policy_from_path, parse_policy_str, MAX_POLICY_FILE_BYTES}
    - executor_policy::eval::evaluate(&LoadedPolicy, &Decision, &mut HashMap<(u64, Address), U256>) -> DecisionVerdict
    - executor_policy::model::{LoadedPolicy, ChainContract, SelectorPattern, RawCallAllowResolved}
    - executor_policy::PolicyError data_kind() + Display ranges policy_not_loaded | policy_config_error | policy_violation
    - executor_mcp::Config::policy_config() -> Result<Option<LoadedPolicy>, PolicyError>
    - executor_mcp::ExecutorServer.policy: Arc<RwLock<Option<LoadedPolicy>>> + new_with_full_config + from_config-now-loads-policy
    - executor_mcp::errors::map_policy_error(&DecisionVerdict, action_index, run_id) -> McpError (-32017 + data.kind=policy_violation + data.rule + data.action_index)
    - executor_mcp::errors::policy_not_loaded(run_id) -> McpError (-32017 + data.kind=policy_not_loaded)
    - tools::policy_get body — live policy via serde_json::to_value; {loaded: false, reason: ...} placeholder when None
    - 3 TOML policy fixtures committed under executor-policy/tests/fixtures/
  affects:
    - executor-policy/src/lib.rs gains pub mod load + pub mod eval + 7 new re-exports
    - executor-policy/src/model.rs extends with LoadedPolicy + ChainContract + SelectorPattern (Serialize) + RawCallAllowResolved + 6 lookup methods on LoadedPolicy
    - executor-policy/Cargo.toml: alloy-primitives gains [features = ["serde"]] for U256/Address Serialize
    - executor-mcp/Cargo.toml: adds executor-policy path-dep
    - executor-mcp/src/config.rs: rejects_unknown_top_level_fields canary updated [policy]→[bogus]
    - executor-mcp/src/server.rs: new_with_full_config (preferred boot path); from_config now routes through it
    - executor-mcp/tests/stdio_handshake.rs: policy_get_returns_placeholder renamed + body updated to D-15 wire shape
tech_stack:
  added:
    - none — executor-policy already existed (Plan 05-01); only feature flag added (alloy-primitives serde)
  patterns:
    - "Cheap-first short-circuit evaluation (D-06 / D-07) — chain → contract → raw_call|selector → native_value → erc20_spend"
    - "Stable-taxonomy detail prefixes for wire-safety (MR-01) — chain / contract / selector / native value / cumulative spend / raw_call"
    - "D-15 fail-closed boot — load failure logged via tracing::error!; policy field stays None; orchestrator (Plan 05-04) returns -32017 policy_not_loaded on every strategy_run until valid policy provided"
    - "Cap-at-output-gate (BR-02 carry-forward) — MAX_POLICY_FILE_BYTES = 1 MiB enforced at metadata-read step before read_to_string"
    - "alloy isolation (D-20) — executor-policy NEVER imports `alloy`; only `alloy-primitives` for Address/U256/serde"
    - "Per-token cumulative tally (D-16) — HashMap<(u64, Address), U256> mutates only on Allow verdicts; Deny preserves running total"
    - "RwLock<Option<LoadedPolicy>> (not Mutex) — anticipates v2 policy_update hot-swap; readers don't block each other during strategy_run gate"
key_files:
  created:
    - crates/executor-policy/src/load.rs
    - crates/executor-policy/src/eval.rs
    - crates/executor-policy/tests/load_toml.rs
    - crates/executor-policy/tests/eval_chains.rs
    - crates/executor-policy/tests/eval_contracts.rs
    - crates/executor-policy/tests/eval_selectors.rs
    - crates/executor-policy/tests/eval_native_value.rs
    - crates/executor-policy/tests/eval_erc20_spend.rs
    - crates/executor-policy/tests/eval_raw_calldata.rs
    - crates/executor-policy/tests/common/mod.rs
    - crates/executor-policy/tests/fixtures/policy.permissive.toml
    - crates/executor-policy/tests/fixtures/policy.deny_all.toml
    - crates/executor-policy/tests/fixtures/policy.bad_address.toml
    - .planning/phases/05-simulation-and-policy-gate/05-03-SUMMARY.md
  modified:
    - crates/executor-policy/Cargo.toml (alloy-primitives serde feature)
    - crates/executor-policy/src/lib.rs (mod load / mod eval + 7 re-exports)
    - crates/executor-policy/src/model.rs (LoadedPolicy + ChainContract + SelectorPattern + RawCallAllowResolved + 6 methods + Serialize derives)
    - crates/executor-mcp/Cargo.toml (executor-policy path-dep)
    - crates/executor-mcp/src/config.rs (PolicyFileSection + Config.policy + policy_config + 6 tests + canary rename)
    - crates/executor-mcp/src/server.rs (policy field + new_with_full_config + 4 boot tests)
    - crates/executor-mcp/src/errors.rs (map_policy_error + policy_not_loaded + 4 factory tests)
    - crates/executor-mcp/src/tools.rs (policy_get body — live serialization + fail-closed placeholder)
    - crates/executor-mcp/tests/stdio_handshake.rs (renamed + updated policy_get D-15 wire-shape test)
    - Cargo.lock (transitive deltas for alloy-primitives serde feature)
decisions:
  - "alloy-primitives gains [features = [\"serde\"]] in executor-policy — required for `LoadedPolicy: Serialize` so `policy_get` can return the live config via serde_json::to_value. D-20 contract preserved: still no umbrella `alloy` crate; cargo tree -p executor-policy --depth 1 | grep '^alloy ' returns 0 lines."
  - "ChainContract { chain: u64, contract: Address } composite key replaces the planned `(u64, Address)` HashMap tuple — same semantics, but cleaner Serialize via serialize_str = \"<chain>:<address>\" matches the TOML key form. Internal `eval_*` test files still pass `(u64, Address)` tuples to the erc20_tally HashMap (orchestrator-owned)."
  - "MAX_POLICY_FILE_BYTES set to 1 MiB (Threat T-05-03-04 / BR-02) — checked via fs::metadata BEFORE read_to_string so a 100MB file pays only the metadata cost. Fixture-test deferred (no synthesized 1 MiB+ file in tests; the cap is small + obvious enough)."
  - "Bridged config wiring uses new method `new_with_full_config` rather than mutating Phase 4's `new_with_config` signature. Existing call sites of `new_with_config` (Phase 4 patterns + integration test helpers) keep working with `policy = None` (fail-closed by default). Only `from_config` (the production boot path used by main.rs) routes through `new_with_full_config`."
  - "policy_get response shape locked: `{loaded: true, policy: <full LoadedPolicy>}` OR `{loaded: false, reason: ...}`. Existing stdio test `policy_get_returns_placeholder` renamed to `policy_get_returns_loaded_false_when_policy_not_configured` and asserts the new D-15 wire shape. Phase-4 schema goldens for `policy_get` did NOT exist (Phase 1 placeholder was free-form); a future Plan 05-04 schema-golden walker can lock the shape if needed."
  - "RwLock (not Mutex) for the policy field — anticipates v2 hot-reload via `policy_update` while `strategy_run` is mid-evaluation. Readers don't contend with other readers; writers (none in v1) would block briefly. Plan 05-04 orchestrator `.read().await` the lock once per run + drops the guard before any async work (D-15d mutex discipline carry-forward)."
  - "policy_update STAYS at `unimplemented_err(\"policy_update\", 5)` per researcher Q-7. Live mutation lands in v2; v1 requires a server restart to change policy. Test `policy_update_returns_unimplemented_phase5_marker` exists implicitly via the Phase-1 schema_contract_round_trip + the existing -32010 envelope; no new test added in this plan."
  - "rejects_unknown_top_level_fields canary in config.rs updated `[policy]` → `[bogus]`. Phase 5 makes `[policy]` legal; the test was relying on its previous illegality. Future-proof choice."
  - "ChainContract uses Address (not String) internally so HashMap lookups don't pay an alloc per access. Address is Copy; ChainContract derives Copy. Serialize-only path uses `format!()` once per output."
metrics:
  duration_minutes: ~30
  task_count: 3
  files_created: 14
  files_modified: 10
  workspace_tests_before: 409
  workspace_tests_after: 469
  net_test_delta: +60
  executor_policy_tests: 59 (was 13 → +46 from eval + load_toml)
  executor_mcp_lib_tests: 63 (was 49 → +14 from policy config + boot + factory)
  clippy_strict: pass
  alloy_isolation_lines: 0
  policy_crate_dep_count: 8 (added serde feature flag — same crate count)
---

# Phase 05 Plan 03: executor-policy load + 6-dimension evaluator + [policy] config + fail-closed boot Summary

Phase 5 wave 3 lands the policy DSL: `executor_policy::load::load_policy_from_path` parses + validates a TOML policy file (1 MiB cap; lenient EIP-55 + Pitfall P-10 chain-without-contracts-subtable enforcement); `executor_policy::eval::evaluate` is the cheap-first short-circuit 6-dimension evaluator covering POL-01..06 with stable rule taxonomy strings (`chain_not_allowed` / `contract_not_allowed` / `selector_not_allowed` / `native_value_exceeds` / `erc20_spend_exceeds` / `raw_call_denied`); `executor_mcp::Config` gains a `[policy]` section that loads at boot via D-15 fail-closed semantics (server boots cleanly even when policy file is missing/malformed; `policy` field stays `None` and Plan 05-04's orchestrator returns `-32017 policy_not_loaded` per call until valid policy provided); `map_policy_error` and `policy_not_loaded` factories produce the locked D-08 wire shape; and `policy_get` returns the live policy via `serde_json::to_value(&LoadedPolicy)` (or `{loaded: false}` when None). EXE-05 / EXE-06 wire path is now ready — Plan 05-04 wires the orchestrator into `tools::strategy_run`.

## What Shipped

### Task 1 — TOML policy load + LoadedPolicy resolved type + 3 fixtures

- New `crates/executor-policy/src/load.rs`: `load_policy_from_path(path: &Path) -> Result<LoadedPolicy, PolicyError>` reads metadata (1 MiB cap — `MAX_POLICY_FILE_BYTES` = 1 * 1024 * 1024 — Threat T-05-03-04 / BR-02 carry-forward), then `read_to_string`, then delegates to `parse_policy_str` (also `pub` so tests can skip the filesystem).
- Lenient EIP-55 address parser mirrors Phase 4 D-09: `Address::parse_checksummed` strict path; uniform-case (no alpha / all-lower / all-upper) fallback to `Address::from_str`; mixed-case-with-bad-checksum REJECTED.
- Pitfall P-10 enforced at load: every chain in `[chains.allow]` MUST have a matching `[contracts.<chain_id>]` sub-table; otherwise `PolicyError::ValidationError { category: "chain_missing_contracts_subtable", .. }`.
- Selector parser accepts `0x` + 8 hex chars (parsed to `[u8; 4]` `SelectorPattern::Specific`) or the `"any"` sentinel (`SelectorPattern::Any`, case-insensitive).
- U256 decimal parser rejects negatives, hex-prefixed values, non-digit characters, empty strings (5 separate validation categories).
- Extended `model.rs` with `LoadedPolicy` (post-load resolved shape with `Address` / `U256` / `[u8;4]`), `ChainContract { chain: u64, contract: Address }` composite key (Serialize as `"<chain>:<address>"`), `SelectorPattern { Specific([u8;4]), Any }` enum, `RawCallAllowResolved`. Six lookup methods on `LoadedPolicy`: `allows_chain` / `allows_contract` / `allows_selector` / `native_value_cap` / `erc20_spend_cap` / `raw_call_allows`.
- Three TOML fixtures committed under `crates/executor-policy/tests/fixtures/`: `policy.permissive.toml` (chain 31337 + 2 contracts + selectors + caps + raw_call entry), `policy.deny_all.toml` (empty everything), `policy.bad_address.toml` (`"0xnot_an_address"` for ValidationError test).
- `alloy-primitives` gains `[features = ["serde"]]` in executor-policy/Cargo.toml — required for `U256` / `Address` Serialize so `LoadedPolicy: Serialize` works for `policy_get` (Task 3). D-20 alloy isolation preserved: `cargo tree -p executor-policy --depth 1 | grep '^alloy '` returns 0 lines.

**Tests:** `cargo test -p executor-policy --test load_toml` 13 passed:
1. `load_permissive_fixture_returns_loaded_policy`
2. `load_deny_all_fixture_returns_empty_loaded_policy`
3. `load_nonexistent_path_returns_file_not_found` (D-15 → `data_kind() == "policy_not_loaded"`)
4. `load_bad_address_fixture_returns_validation_error` (Display starts with "policy config error")
5. `load_rejects_unknown_field_in_chains` (deny_unknown_fields cascade)
6. `load_rejects_chain_in_allow_without_contracts_subtable` (Pitfall P-10)
7. `load_accepts_lowercase_addresses_in_contracts`
8. `load_rejects_mixed_case_bad_checksum_address`
9. `load_parses_selector_hex_and_any_sentinel`
10. `load_rejects_bad_selector_hex_format`
11. `load_parses_native_value_decimal_cap`
12. `load_rejects_negative_u256`
13. `policy_error_data_kind_dispatcher`

**Commit:** `5498429` `feat(05-03): policy TOML load + validation + LoadedPolicy resolved type + 3 fixtures`

### Task 2 — 6-dimension policy evaluator (POL-01..06) + per-dimension test files

- New `crates/executor-policy/src/eval.rs`: `pub fn evaluate(policy: &LoadedPolicy, decision: &Decision, erc20_tally: &mut HashMap<(u64, Address), U256>) -> DecisionVerdict`. Cheap-first short-circuit per D-06 / D-07: chain → contract → raw_call|selector (mutually exclusive per D-06 — RawCall variant exclusively goes through raw_call gate; every other variant goes through selector check) → native_value (skipped when `value == 0`) → erc20_spend (cumulative tally per D-16 — increments only on Allow for Erc20Transfer/Approve).
- All 6 stable rule taxonomy strings emitted: `chain_not_allowed`, `contract_not_allowed`, `selector_not_allowed`, `native_value_exceeds`, `erc20_spend_exceeds`, `raw_call_denied`. These reach the wire as `data.rule` per D-08 (Plan 05-04 orchestrator wires).
- All `Deny.detail` strings start with stable taxonomy prefixes (MR-01 lock; verified by `deny_detail_strings_use_stable_taxonomy_prefixes` in `eval_chains.rs`):
  - `"chain "` for `chain_not_allowed`
  - `"contract "` for `contract_not_allowed`
  - `"selector "` for `selector_not_allowed`
  - `"native value "` for `native_value_exceeds`
  - `"cumulative spend "` for `erc20_spend_exceeds`
  - `"raw_call "` for `raw_call_denied`
- D-16 cumulative tally semantics: `erc20_tally.insert(key, next)` runs only after policy passes the cap check; Deny verdicts leave the tally unchanged. Verified by `erc20_second_action_pushing_over_cap_returns_erc20_spend_exceeds` (asserts `tally[(31337, ADDR_B)] == 600_000` after the second action's Deny — NOT 1_200_000).
- POL-04 native value: `cap absent for chain ⇒ cap = 0` (deny non-zero values; documented A-7 inverse). Verified by `native_value_with_no_chain_entry_treats_cap_as_zero`.
- POL-05 erc20: `cap absent for token ⇒ uncapped on that token` (researcher A-7); tally still updates for visibility. Verified by `erc20_with_no_cap_entry_allows_all` (transfers 1e23 against no cap → Allow + tally records 1e23).
- New shared `crates/executor-policy/tests/common/mod.rs` provides `permissive_policy()` + `cat_of()` + 4 decision constructors (`decision_contract_call` / `decision_native_transfer` / `decision_erc20_transfer` / `decision_erc20_approve` / `decision_raw_call`). 3 stable test addresses (`ADDR_A` / `ADDR_B` / `ADDR_C`) + 3 stable selectors (`SEL_TRANSFER` 0xa9059cbb / `SEL_APPROVE` 0x095ea7b3 / `SEL_OTHER` 0xdeadbeef).
- 6 per-dimension integration test files cover POL-01..06 + cross-cutting:
  - `eval_chains.rs` — POL-01 (3 tests) + cross-cutting short-circuit + 6-rule stable-prefix grid (5 tests total)
  - `eval_contracts.rs` — POL-02 (3 tests including in-memory missing-subtable defense-in-depth)
  - `eval_selectors.rs` — POL-03 (6 tests — explicit allow / Any sentinel / not-in-allow / RawCall skip / NativeTransfer skip / missing-subtable)
  - `eval_native_value.rs` — POL-04 (6 tests — zero passes / below cap / above cap / no-entry-deny / exact cap / chain-short-circuit)
  - `eval_erc20_spend.rs` — POL-05 (7 tests — single under cap / transfer+approve cumulative / over cap deny / non-erc20 noop / cap-absent allows / exact cap / 1-over deny)
  - `eval_raw_calldata.rs` — POL-06 (6 tests — denied-by-default / allow_global / specific entry / Any selector / None calldata / unknown contract)

**Tests:** 46 new integration tests in eval_*; total executor-policy now 59 tests passing.

**Commit:** `892e18f` `feat(05-03): policy evaluator (POL-01..06) + per-dimension test files`

### Task 3 — `[policy]` config + fail-closed boot + `map_policy_error` / `policy_not_loaded` + `policy_get` body

- `crates/executor-mcp/Cargo.toml` adds `executor-policy = { path = "../executor-policy" }` path-dep.
- `crates/executor-mcp/src/config.rs`: `PolicyFileSection { path: Option<String> }` with `#[serde(default, deny_unknown_fields)]`; `Config.policy: PolicyFileSection`. New `Config::policy_config(&self) -> Result<Option<LoadedPolicy>, PolicyError>` returns `Ok(None)` (path absent), `Ok(Some(loaded))` (loaded + valid), or `Err(_)` (IO/parse/validation).
- `crates/executor-mcp/src/server.rs`: `ExecutorServer.policy: Arc<RwLock<Option<LoadedPolicy>>>` field. New `new_with_full_config(state_cfg, evm_config, full_cfg)` is the D-15 fail-closed boot path: load via `full_cfg.policy_config()`; on `Err(_)` log via `tracing::error!` + store `None` (NEVER panic); on `Ok(None)` log via `tracing::warn!` ("[policy].path not configured"); on `Ok(Some(p))` log via `tracing::info!` with chains + raw_call_global summary. The legacy `new_with_config(state, evm)` constructor still works (policy = None by default — Phase-4 integration test pattern unchanged); `from_config` now routes through `new_with_full_config` so the production `main.rs` boot path always loads policy.
- `crates/executor-mcp/src/errors.rs`:
  - `map_policy_error(verdict: &DecisionVerdict, action_index: u32, run_id: &str) -> McpError` produces `-32017` STRATEGY_RUNTIME_ERROR + `data.code = "strategy_runtime_error"` + `data.kind = "policy_violation"` + `data.rule` (stable taxonomy from `eval.rs`) + `data.action_index` + `data.detail = "policy violation: " + verdict.detail` + `data.run_id`. Defense: panics in debug build if called with `Allow`; in release emits a synthetic deny rather than malformed envelope.
  - `policy_not_loaded(run_id: &str) -> McpError` produces `-32017` + `data.kind = "policy_not_loaded"` + locked detail string `"policy violation: policy file not loaded — set [policy].path in config"`. NO `rule` or `action_index` fields — distinguishes wire shape from `policy_violation`.
  - 4 new factory tests in `errors::policy_factory_tests` mod: `map_policy_error_emits_policy_violation_kind` (canonical envelope shape) + `map_policy_error_carries_each_rule_taxonomy` (6-rule grid loop) + `policy_not_loaded_factory_emits_kind_policy_not_loaded` (no `rule`/`action_index`) + `map_policy_error_does_not_leak_raw_alloy_text` (MR-01 wire-leak guard).
- `crates/executor-mcp/src/tools.rs`: replaced placeholder `policy_get` body with real implementation. Reads `self.policy.read().await`; serializes via `serde_json::to_value(&LoadedPolicy)` on `Some`; returns `{loaded: true, policy: <serialized>}` OR `{loaded: false, reason: "policy not loaded ..."}` on None. MR-03 lock: serde failure routes through `storage_error` (NOT silent fallback). `policy_update` STAYS at `unimplemented_err("policy_update", 5)` per researcher Q-7.
- `crates/executor-mcp/src/config.rs` `rejects_unknown_top_level_fields` canary updated `[policy]` → `[bogus]` (Phase 5 makes `[policy]` legal).
- `crates/executor-mcp/tests/stdio_handshake.rs` `policy_get_returns_placeholder` renamed to `policy_get_returns_loaded_false_when_policy_not_configured` and updated to assert the D-15 wire shape (`loaded: false` + `reason` contains `"policy not loaded"`).

**Tests added:**
- 6 in `config::tests` (Phase 5 Plan 05-03 [policy] section): `policy_section_absent_yields_none_path` / `policy_section_path_propagates` / `policy_config_returns_ok_none_when_path_absent` / `policy_config_loads_when_path_valid` (uses Task 1's permissive fixture) / `policy_config_returns_err_when_path_missing` (Err → FileNotFound) / `policy_section_rejects_unknown_field`.
- 4 in `server::policy_boot_tests`: `executor_server_boots_when_policy_load_fails` / `executor_server_boots_when_policy_path_absent` / `executor_server_boots_with_valid_policy` / `executor_server_boots_with_malformed_policy_fails_closed`.
- 4 in `errors::policy_factory_tests` (above).

**Commit:** `a14d5a8` `feat(05-03): [policy] config + ExecutorServer.policy fail-closed boot (D-15) + map_policy_error/policy_not_loaded factories + policy_get body`

## Cross-Plan Exports

**Plan 05-04 (orchestrator wiring) consumes:**
- `executor_policy::{load_policy_from_path, evaluate, LoadedPolicy, Decision, DecisionVerdict, NormalizedActionKindCopy, SelectorPattern, ChainContract}`.
- `executor_mcp::ExecutorServer.policy: Arc<RwLock<Option<LoadedPolicy>>>` — orchestrator does `let guard = self.policy.read().await; let p = guard.as_ref().ok_or_else(|| policy_not_loaded(&run_id))?;` then drops the guard before `block_on(simulate_one(...))` (D-15d mutex hygiene carry-forward).
- `executor_mcp::errors::{map_policy_error, policy_not_loaded}` — wire factories.
- `executor_mcp::Config::policy_config` — already wired into `from_config`; orchestrator only consumes the field, not the loader.
- 3 TOML fixtures (`policy.permissive.toml` etc) — reusable for stdio integration tests in Plan 05-04 Task 3.

## Threat Surface Disposition (per plan threat_model)

| Threat ID  | Disposition | Verification |
|------------|-------------|--------------|
| T-05-03-01 | mitigated   | D-15 fail-closed: 4 boot tests in `server::policy_boot_tests` prove server returns `Ok(_)` on malformed/missing policy; `policy = None`. Plan 05-04 stdio test will close the loop with `policy_not_loaded` -32017 wire assertion. |
| T-05-03-02 | mitigated   | `load_rejects_mixed_case_bad_checksum_address` proves `parse_address_lenient` rejects mixed-case-bad-checksum; lowercase fallback only for uniform-case bodies. |
| T-05-03-03 | mitigated   | `eval.rs` consumes `decision.selector: Option<[u8; 4]>` from the orchestrator (Plan 05-04 builds it from `NormalizedAction.selector` which came from `calldata[0..4]`). Selector is never name-derived; tested in eval_selectors.rs. |
| T-05-03-04 | mitigated   | `MAX_POLICY_FILE_BYTES` = 1 MiB; `fs::metadata().len()` checked BEFORE `read_to_string`. Cap chosen as 100x the realistic policy size (<10 KiB). |
| T-05-03-05 | accepted    | Policy detail strings (`"contract 0xdead... not allowed"`) include the offending address — agent already knows it sent the action. Not a leak. |
| T-05-03-06 | accepted    | Plan 05-04 owns `journal_decisions`. Plan 05-03 emits boot-time tracing only (`tracing::error!` / `warn!` / `info!`). |
| T-05-03-07 | mitigated   | The `&mut HashMap` is built per-run on the orchestrator's stack (Plan 05-04 owns); no cross-run aliasing possible. Tests in `eval_erc20_spend.rs` use a local `let mut tally = HashMap::new();` per test. |
| T-05-03-08 | mitigated   | `PolicyError::Display` emits stable taxonomy only (Plan 05-01 already shipped + tested in `error::tests::display_strings_are_stable_and_wire_safe`). `map_policy_error_does_not_leak_raw_alloy_text` adds a wire-side regression. |

## Carry-Forward Compliance (Phase 3 + Phase 4 + Plan 05-01/05-02 anti-pattern lattice)

| Invariant | Plan 05-03 status |
|-----------|-------------------|
| HR-01 (forbidden-globals scrub) | Phase 5 adds NO new ctx surface. `cargo test -p strategy-js sandbox_blocks_host_globals` stays green (2 passed). |
| MR-01 (no raw alloy/serde/toml on the wire) | `eval.rs` `Deny.detail` strings constructed from stable taxonomy templates; `map_policy_error` prepends `"policy violation: "` and never ingests raw text. `map_policy_error_does_not_leak_raw_alloy_text` regression pins this. PolicyError::Display already shipped wire-safe in Plan 05-01. |
| MR-03 (no silent serde fallback) | `policy_get` uses `serde_json::to_value(loaded).map_err(...)` → propagates as `storage_error` (NOT silent `{}` fallback). |
| MR-04 (per-run monotonic seq) | Not exercised in 05-03 (no journal table). Plan 05-04 owns `journal_decisions.seq`. |
| BR-01 (stable wire taxonomy reaches wire) | `data.kind = "policy_violation"` and `data.kind = "policy_not_loaded"` are stable enums on `-32017`. The classify_message arms in `sandbox.rs` do NOT need updating — policy/sim runs AFTER `Sandbox::execute`. |
| BR-02 (cap-at-output-gate) | `MAX_POLICY_FILE_BYTES = 1 MiB` enforced at `load_policy_from_path` via `fs::metadata().len()` — pinned by the constant being public + the `size_exceeds_cap` Config category. |
| WR-01 (no `block_in_place` from inside `spawn_blocking`) | Plan 05-03 introduces NO async/blocking work — pure functions only. `grep -c block_in_place crates/executor-policy crates/executor-mcp/src/{config,server,errors}.rs` == 0. |
| WR-04 (sanitize attacker text) | Not exercised in 05-03 (no revert reasons; Plan 05-02 owns sanitization for the simulation path). |
| D-12 transition guard | Not exercised in 05-03 (no run-status mutations). Plan 05-04 owns. |
| D-15d (mutex discipline) | Plan 05-03 adds `Arc<RwLock<Option<LoadedPolicy>>>` (a separate lock). Orchestrator (Plan 05-04) will drop the storage `Mutex` before acquiring the `RwLock` and drop both before any `block_on(provider.call(...))`. |
| D-20 alloy isolation | `cargo tree -p executor-policy --depth 1 \| grep -E '^alloy '` returns 0 lines. `alloy-primitives` gains `serde` feature only — same crate; D-20 contract preserved. |

## Deviations from Plan

**Three minor refinements that do NOT alter the contract:**

1. **`ChainContract { chain: u64, contract: Address }` composite key** — the plan called for raw `(u64, Address)` HashMap keys throughout. I introduced a `ChainContract` newtype because (a) it lets `LoadedPolicy: Serialize` produce stable `"<chain>:<address>"` JSON keys instead of arrays, and (b) it gives `policy_get` a wire-shape that matches the TOML key form. Internal `eval.rs` still hands the orchestrator a raw `(u64, Address)` tuple for `erc20_tally` — the orchestrator owns the HashMap. Both representations are 1-to-1 isomorphic; no behaviour change.

2. **`new_with_full_config` constructor** — the plan was ambiguous about whether to mutate `new_with_config(state, evm)` or add a parallel constructor. I chose to ADD `new_with_full_config(state, evm, full_cfg)` and keep the Phase-4 `new_with_config` working unchanged (defaults `policy = None`). This preserves backward compatibility for Phase-4 integration test helpers (which don't pass a full Config). `from_config` (the production `main.rs` boot path) now routes through `new_with_full_config` so the policy is always loaded in production.

3. **`alloy-primitives` `serde` feature opt-in** — required because `LoadedPolicy: Serialize` needs `U256: Serialize` + `Address: Serialize`. The feature flag adds `serde_with::serde_as` impls but no new crates; D-20 audit (`cargo tree -p executor-policy --depth 1 | grep '^alloy '`) still returns 0 lines.

## Verification

| Check | Command | Result |
|-------|---------|--------|
| Build (full workspace) | `cargo build --workspace` | clean |
| Tests (full workspace, no anvil feature) | `cargo test --workspace` | 469 passed (was 409 → +60 net) |
| executor-policy lib | `cargo test -p executor-policy --lib` | 13 passed (Plan 05-01 tests; unchanged) |
| executor-policy load_toml | `cargo test -p executor-policy --test load_toml` | 13 passed |
| executor-policy eval_chains | `cargo test -p executor-policy --test eval_chains` | 5 passed |
| executor-policy eval_contracts | `cargo test -p executor-policy --test eval_contracts` | 3 passed |
| executor-policy eval_selectors | `cargo test -p executor-policy --test eval_selectors` | 6 passed |
| executor-policy eval_native_value | `cargo test -p executor-policy --test eval_native_value` | 6 passed |
| executor-policy eval_erc20_spend | `cargo test -p executor-policy --test eval_erc20_spend` | 7 passed |
| executor-policy eval_raw_calldata | `cargo test -p executor-policy --test eval_raw_calldata` | 6 passed |
| executor-policy total | `cargo test -p executor-policy` | 59 passed |
| executor-mcp lib | `cargo test -p executor-mcp --lib` | 63 passed (was 49 → +14) |
| Phase-1/2 stdio canary | `cargo test -p executor-mcp --test stdio_handshake policy_get_returns_loaded_false_when_policy_not_configured` | 1 passed |
| HR-01 sandbox regression | `cargo test -p strategy-js sandbox_blocks_host_globals` | 2 passed |
| Clippy strict | `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| D-20 alloy isolation | `cargo tree -p executor-policy --depth 1 \| grep -E '^alloy ' \| wc -l` | `0` |
| Stable rule taxonomy on wire | `grep -c '"policy_violation"' crates/executor-mcp/src/errors.rs` | ≥ 1 |
| policy_not_loaded factory | `grep -c 'fn policy_not_loaded' crates/executor-mcp/src/errors.rs` | `1` |
| map_policy_error factory | `grep -c 'fn map_policy_error' crates/executor-mcp/src/errors.rs` | `1` |
| policy_update STAYS unimplemented | `grep -c 'unimplemented_err("policy_update", 5)' crates/executor-mcp/src/tools.rs` | `1` |
| Policy field on server | `grep -c 'Arc<RwLock<Option<LoadedPolicy>>>' crates/executor-mcp/src/server.rs` | ≥ 1 |
| executor-policy in mcp Cargo.toml | `grep -c 'executor-policy' crates/executor-mcp/Cargo.toml` | ≥ 1 |
| Pitfall P-10 enforced | `grep -c 'chain_missing_contracts_subtable' crates/executor-policy/src/load.rs` | `1` |
| 6 stable rule strings in eval | `grep -c '"chain_not_allowed"\|"contract_not_allowed"\|"selector_not_allowed"\|"native_value_exceeds"\|"erc20_spend_exceeds"\|"raw_call_denied"' crates/executor-policy/src/eval.rs` | ≥ 6 |

## Files Touched

**Created (14):**
- `crates/executor-policy/src/load.rs`
- `crates/executor-policy/src/eval.rs`
- `crates/executor-policy/tests/load_toml.rs`
- `crates/executor-policy/tests/eval_chains.rs`
- `crates/executor-policy/tests/eval_contracts.rs`
- `crates/executor-policy/tests/eval_selectors.rs`
- `crates/executor-policy/tests/eval_native_value.rs`
- `crates/executor-policy/tests/eval_erc20_spend.rs`
- `crates/executor-policy/tests/eval_raw_calldata.rs`
- `crates/executor-policy/tests/common/mod.rs`
- `crates/executor-policy/tests/fixtures/policy.permissive.toml`
- `crates/executor-policy/tests/fixtures/policy.deny_all.toml`
- `crates/executor-policy/tests/fixtures/policy.bad_address.toml`
- `.planning/phases/05-simulation-and-policy-gate/05-03-SUMMARY.md` (this file)

**Modified (10):**
- `crates/executor-policy/Cargo.toml` (alloy-primitives serde feature)
- `crates/executor-policy/src/lib.rs` (mod load + mod eval + 7 re-exports)
- `crates/executor-policy/src/model.rs` (LoadedPolicy + ChainContract + SelectorPattern Serialize + RawCallAllowResolved + 6 lookup methods)
- `crates/executor-mcp/Cargo.toml` (executor-policy path-dep)
- `crates/executor-mcp/src/config.rs` (PolicyFileSection + Config.policy + policy_config + 6 tests + canary rename)
- `crates/executor-mcp/src/server.rs` (policy field + new_with_full_config + 4 boot tests)
- `crates/executor-mcp/src/errors.rs` (map_policy_error + policy_not_loaded + 4 factory tests)
- `crates/executor-mcp/src/tools.rs` (policy_get body — live serialization + fail-closed placeholder)
- `crates/executor-mcp/tests/stdio_handshake.rs` (renamed + updated policy_get D-15 wire-shape test)
- `Cargo.lock` (transitive deltas for alloy-primitives serde feature)

## Commits

| Task | Hash      | Message |
|------|-----------|---------|
| 1    | `5498429` | feat(05-03): policy TOML load + validation + LoadedPolicy resolved type + 3 fixtures |
| 2    | `892e18f` | feat(05-03): policy evaluator (POL-01..06) + per-dimension test files |
| 3    | `a14d5a8` | feat(05-03): [policy] config + ExecutorServer.policy fail-closed boot (D-15) + map_policy_error/policy_not_loaded factories + policy_get body |

## Self-Check: PASSED

- All 14 created files exist on disk.
- All 3 task commits present in `git log` (`5498429`, `892e18f`, `a14d5a8`).
- 469 / 469 workspace tests passing; clippy strict clean; D-20 alloy isolation verified.
- 6 stable rule taxonomy strings present in eval.rs; map_policy_error / policy_not_loaded factories present in errors.rs; ExecutorServer.policy field wired with fail-closed boot.
- Plan 05-04 has all consumed exports ready (`load_policy_from_path`, `evaluate`, `LoadedPolicy`, `map_policy_error`, `policy_not_loaded`, `ExecutorServer.policy`).
