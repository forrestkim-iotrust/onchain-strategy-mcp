---
phase: 05
slug: simulation-and-policy-gate
status: ready
nyquist_compliant: true
wave_0_complete: false
created: 2026-04-27
updated: 2026-04-27
---

# Phase 05 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution. Mirrors `04-VALIDATION.md` shape; populated from each PLAN.md `<verification><automated>` block + acceptance criteria.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (Rust built-in) + `tokio::test(flavor = "multi_thread")` for stdio + `--features anvil-tests` for alloy-spawned anvil integration |
| **Config file** | `Cargo.toml` workspace + per-crate `[dev-dependencies]` + `executor-evm/Cargo.toml [features]` (carry-forward Phase 4) + new `crates/executor-policy/Cargo.toml` |
| **Quick run command** | `cargo test --workspace --lib && cargo test -p executor-policy && cargo test -p executor-evm --test normalize && cargo test -p executor-state --test journal_decision_seq` |
| **Anvil-gated command** | `cargo test --workspace --features anvil-tests` |
| **Full suite command** | `cargo test --workspace --features anvil-tests` |
| **Estimated runtime** | ~30–60 seconds (default ~10s; anvil-gated subset adds ~30–50s for anvil spawn + per-test deploys). Phase-4 baseline ~353 tests. Phase-5 net additions: ~30 executor-policy unit + ~11 normalize + ~4 simulate (anvil) + ~4 journal_decision_seq + ~2 run_lifecycle_transition extensions + ~6 server/config/errors lib tests + ~10 stdio tests + ~6 schema goldens ≈ 75 new. Workspace target ~430 tests after Phase 5. |

---

## Sampling Rate

- **After every task commit:** `cargo test -p <crate-touched>` (≤ 15s default; ≤ 60s with anvil if relevant).
- **After every plan wave:** `cargo test --workspace`. With anvil gates, also `--features anvil-tests` once per wave on machines with foundry.
- **Before `/gsd-verify-work`:** Full suite + `cargo clippy --workspace --all-targets -- -D warnings` must be green.
- **Max feedback latency:** ~60 seconds (clippy + tests + anvil-gated reads + sims).

---

## Per-Task Verification Map

*Populated from each PLAN.md `<verification><automated>` block. File-Exists column reflects intent: `✅` if the test file already exists in the repo; `❌ W0` if the same plan's first task creates it.*

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 05-01-T1 | 05-01 | 1 | EXE-02 | T-05-01-04 (selector golden), T-05-01-07 (refactor drift) | executor-policy crate scaffold (alloy-FREE) + ERC20_WRITE_ABI + extract encode_call_input | unit (rust) | `cargo build -p executor-policy && cargo test -p executor-policy --lib && cargo build -p executor-evm && cargo test -p executor-evm --lib && cargo clippy -p executor-policy -p executor-evm --all-targets -- -D warnings && [ "$(cargo tree -p executor-policy --depth 1 \| grep -E '^alloy ' \| wc -l \| tr -d ' ')" = "0" ]` | ❌ W0 (whole crate is new + ERC20_WRITE_ABI + encode_call_input extracted from action.rs) | ⬜ pending |
| 05-01-T2 | 05-01 | 1 | EXE-02 | T-05-01-02 (encode error propagation), T-05-01-03 (BR-01 wire-safety on bad input) | Action → NormalizedAction normalize layer (per-variant table per D-02) | unit (rust) | `cargo test -p executor-evm --test normalize && cargo test -p executor-evm --lib && cargo clippy -p executor-evm --all-targets -- -D warnings` | ❌ W0 (normalize.rs + tests/normalize.rs new) | ⬜ pending |
| 05-01-T3 | 05-01 | 1 | EXE-01 | T-05-01-01 (DoS via 10000 actions), BR-02 carry-forward | MAX_ACTIONS_PER_RUN=32 cap at validate_strategy_output | unit + stdio | `cargo test -p executor-mcp --lib validation::tests::max_actions_per_run_constant_is_32 && cargo test -p executor-mcp --test stdio_handshake strategy_run_caps_action_array_length_at_32 && cargo build -p executor-mcp && cargo clippy -p executor-mcp --all-targets -- -D warnings` | ❌ W0 (constant new + stdio test new) | ⬜ pending |
| 05-02-T1 | 05-02 | 2 | EXE-03, EXE-04 | T-05-02-01 (revert sanitize / WR-04), T-05-02-03 (bad-checksum simulation_from / D-14), T-05-02-04 (timeout DoS) | simulate_one + SimulationOutcome enum + sanitize_revert_reason promotion + EvmConfig.simulation_from + lenient validation | unit + integration (no anvil) | `cargo test -p executor-evm --lib config::tests simulate::tests read::tests::sanitize_revert_reason && cargo test -p executor-evm --test simulate_timeout && cargo build -p executor-evm && cargo clippy -p executor-evm --all-targets -- -D warnings` | ❌ W0 (simulate.rs + simulate_timeout test new) | ⬜ pending |
| 05-02-T2 | 05-02 | 2 | EXE-03, EXE-04 | T-05-02-01 (revert sanitize against real anvil), T-05-02-02 (revert spoofing) | anvil-gated simulate tests (pass / revert / transport / timeout) + revert_counter bytecode | integration (anvil) | `cargo test -p executor-evm --features anvil-tests --test simulate_anvil` | ❌ W0 (simulate_anvil.rs + revert_counter.hex new) | ⬜ pending |
| 05-02-T3 | 05-02 | 2 | EXE-04 | T-05-02-03 (bad-checksum at config layer), T-05-02-07 (raw_for_log leakage) | [evm.simulation_from] config + map_simulation_error factory + stdio test stub (#[ignore] for Plan 05-04) | unit + stdio (registered) | `cargo test -p executor-mcp --lib config::tests::evm_section_default_simulation_from_is_anvil_account_0 config::tests::evm_section_simulation_from_override_is_propagated config::tests::evm_section_simulation_from_bad_checksum_returns_err_at_evm_config errors::sim_factory_tests && cargo build -p executor-mcp && cargo clippy -p executor-mcp --all-targets -- -D warnings` | ❌ W0 (config + errors + stub new) | ⬜ pending |
| 05-03-T1 | 05-03 | 3 | POL-01..06 | T-05-03-02 (case-folding bypass), T-05-03-04 (policy file size DoS) | executor-policy::load + LoadedPolicy resolved type + 3 TOML fixtures | unit (rust) | `cargo test -p executor-policy --test load_toml && cargo build -p executor-policy && cargo clippy -p executor-policy --all-targets -- -D warnings` | ❌ W0 (load.rs + LoadedPolicy + 3 fixture files new) | ⬜ pending |
| 05-03-T2 | 05-03 | 3 | POL-01, POL-02, POL-03, POL-04, POL-05, POL-06 | T-05-03-03 (selector-collision via name spoofing — selectors come from raw bytes, never from name), T-05-03-07 (erc20_tally race) | executor-policy::eval — 6-dimension deny-by-default evaluator + per-dimension test files | unit (rust) | `cargo test -p executor-policy --test eval_chains --test eval_contracts --test eval_selectors --test eval_native_value --test eval_erc20_spend --test eval_raw_calldata && cargo test -p executor-policy --lib && cargo clippy -p executor-policy --all-targets -- -D warnings` | ❌ W0 (eval.rs + 6 per-dimension test files new) | ⬜ pending |
| 05-03-T3 | 05-03 | 3 | POL-01..06 (wire path) | T-05-03-01 (default-allow when policy missing — D-15 fail-closed), T-05-03-08 (PolicyError display leakage) | [policy] config + ExecutorServer.policy fail-closed boot + map_policy_error/policy_not_loaded factories + policy_get body | unit + integration | `cargo test -p executor-mcp --lib config::tests::policy_section_absent_yields_none_path config::tests::policy_section_path_propagates config::tests::policy_config_returns_ok_none_when_path_absent config::tests::policy_config_loads_when_path_valid config::tests::policy_config_returns_err_when_path_missing server::policy_boot_tests errors::policy_factory_tests && cargo build -p executor-mcp && cargo clippy -p executor-mcp --all-targets -- -D warnings` | ❌ W0 (config section + server field + errors factories new) | ⬜ pending |
| 05-04-T1 | 05-04 | 4 | STJ-05 | T-05-04-02 (journal tampering — accepted), T-05-04-03 (audit trail integrity / MR-04) | journal_decisions table + record_decision repo + phase5_emittable rename + transition table extension + chain_id OnceCell | unit (rust) | `cargo test -p executor-state --test journal_decision_seq && cargo test -p executor-state --test run_lifecycle_transition && cargo test -p executor-core --lib schema::execution && cargo test -p executor-mcp --lib server::chain_id_tests && cargo build --workspace && cargo clippy -p executor-state -p executor-core -p executor-mcp --all-targets -- -D warnings && [ "$(grep -rc 'phase3_emittable' crates/ \| grep -v ':0$' \| wc -l \| tr -d ' ')" = "0" ]` | ❌ W0 (journal_decisions schema + record_decision + journal_decision_seq.rs + RunStatus transition tests new; phase3_emittable→phase5_emittable rename) | ⬜ pending |
| 05-04-T2 | 05-04 | 4 | EXE-03, EXE-04, EXE-05, EXE-06, STJ-05 | T-05-04-01 (gate bypass), T-05-04-06 (mutex contention / D-15d), T-05-04-07 (BR-01 reclassify-as-exception bypass) | tools::strategy_run pipeline (cap → normalize → policy → simulate → journal_actions → transition) + journal://{run_id} body extension + un-ignore Plan 05-02 stub | integration (mixed) | `cargo test -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_returns_simulation_failed_when_revert strategy_run_emits_decisions_when_actions_pass_policy_and_simulation && cargo test -p executor-mcp --test stdio_handshake strategy_run_returns_policy_not_loaded_when_policy_missing && cargo build -p executor-mcp && cargo clippy -p executor-mcp --all-targets -- -D warnings && [ "$(grep -rc 'block_in_place' crates/executor-mcp/src/tools.rs \| grep -v ':0$' \| wc -l \| tr -d ' ')" = "0" ]` | ✅ (extends Phase-4 tools.rs + Phase-3 stdio_handshake.rs) | ⬜ pending |
| 05-04-T3 | 05-04 | 4 | EXE-03..06, POL-01..06, STJ-05 (full coverage) | T-05-04-04 (data.detail attacker text leak), T-05-04-05 (DoS via 32 reverting actions), T-05-04-08 (journal payload leak — accepted) | comprehensive stdio negative grid (≥10 tests) + 6 schema goldens + REQUIREMENTS/ROADMAP updates | integration (anvil) + golden | `cargo test -p executor-mcp --features anvil-tests --test stdio_handshake -- strategy_run_returns_policy_violation_for strategy_run_returns_simulation_failed_when_revert strategy_run_returns_policy_not_loaded_when_policy_missing strategy_run_journal_records_pass_decisions_on_success strategy_run_journal_records_fail_decision_on_policy_denied strategy_run_records_skipped_simulation_when_policy_denied strategy_run_policy_violation_data_kind_is_policy_violation_not_exception && cargo test -p executor-mcp schema_goldens_match_round_trip && cargo test --workspace` | ❌ W0 (10+ stdio tests + 6 goldens new) | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

**Sampling continuity:** Every task has an `<automated>` cargo command. No 3 consecutive tasks lack automated verify. Wave 1 covers 3 tasks, Wave 2 covers 3, Wave 3 covers 3, Wave 4 covers 3. Total feedback latency for full default suite stays ≤ ~15s; with anvil gates ≤ ~60s.

---

## Wave 0 Requirements

Wave 0 fixture/test files are created **as part of the same plan that needs them** (no separate Wave 0 plan — Phase 5 scope folds Wave 0 into each plan's first task). The full Wave 0 set:

- [ ] `crates/executor-policy/Cargo.toml` + `src/{lib,model,error,decision,selector}.rs` — created by Plan **05-01 Task 1**.
- [ ] `crates/executor-policy/src/{load,eval}.rs` — created by Plan **05-03 Task 1+2**.
- [ ] `crates/executor-policy/tests/{load_toml.rs, eval_chains.rs, eval_contracts.rs, eval_selectors.rs, eval_native_value.rs, eval_erc20_spend.rs, eval_raw_calldata.rs}` — created by Plan **05-03 Task 1+2**.
- [ ] `crates/executor-policy/tests/fixtures/{policy.permissive.toml, policy.deny_all.toml, policy.bad_address.toml}` — Plan **05-03 Task 1**.
- [ ] `crates/executor-evm/src/normalize.rs` + `tests/normalize.rs` — Plan **05-01 Task 2**.
- [ ] `crates/executor-evm/src/simulate.rs` + `tests/simulate_anvil.rs` + `tests/simulate_timeout.rs` + `tests/fixtures/revert_counter.hex` + `tests/fixtures/revert_counter.sol-src.txt` — Plan **05-02 Task 1+2**.
- [ ] `crates/executor-state/tests/journal_decision_seq.rs` — Plan **05-04 Task 1**.
- [ ] `crates/executor-core/tests/schemas/{StrategyRunResponse.json (regen), StrategyOutcome.json (regen), Decision.json, PolicyVerdict.json, SimulationOutcome.json, PolicyConfig.json}` — Plan **05-04 Task 3** via `UPDATE_SCHEMAS=1`.
- [ ] Root `Cargo.toml` — `members` array updated to include `crates/executor-policy`. Updated by Plan **05-01 Task 1**.
- [ ] `crates/executor-mcp/Cargo.toml` — `executor-policy` path-dep added. Updated by Plan **05-03 Task 3**.

`wave_0_complete: true` is set by the executor when all items exist on disk and their owning task's `<automated>` command exits 0.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| _none expected_ | | | All Phase-5 surfaces are observable via `cargo test --workspace [--features anvil-tests]`. |

The `--features anvil-tests` integration tests require `anvil` (foundry) on PATH. Phase-4 D-14 carry-forward — fixture skips cleanly when anvil missing (no panic). For developers without foundry:
1. Install: `curl -L https://foundry.paradigm.xyz | bash && foundryup`.
2. OR set `ANVIL_RPC_URL` env var pointing at an externally-managed devnet.
3. OR skip anvil-gated tests in default `cargo test --workspace`.

---

## Phase Requirements → Test Map

| Req ID | Behavior | Owning Plan / Task | Automated Command | File Exists |
|--------|----------|-------------------|-------------------|-------------|
| **EXE-01** | Runtime validates Action[] before any simulation or signing | 05-01 T3 (MAX_ACTIONS_PER_RUN cap at validate_strategy_output — supplements Phase-4 validate) | `cargo test -p executor-mcp --test stdio_handshake strategy_run_caps_action_array_length_at_32 && cargo test -p executor-mcp --lib validation::tests::max_actions_per_run_constant_is_32` | ❌ W0 |
| **EXE-02** | Runtime ABI-encodes contract call actions into transaction requests | 05-01 T1 (encode_call_input + ERC20_WRITE_ABI selector goldens) + 05-01 T2 (normalize per-variant table) | `cargo test -p executor-evm --test normalize && cargo test -p executor-evm --lib erc20::tests::erc20_write_abi_transfer_selector_is_a9059cbb erc20::tests::erc20_write_abi_approve_selector_is_095ea7b3` | ❌ W0 |
| **EXE-03** | Runtime simulates transaction requests before signing | 05-02 T1 (simulate_one adapter) + 05-02 T2 (anvil-gated pass/revert/transport) + 05-04 T2 (orchestration into strategy_run) | `cargo test -p executor-evm --features anvil-tests --test simulate_anvil && cargo test -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_emits_decisions_when_actions_pass_policy_and_simulation` | ❌ W0 |
| **EXE-04** | Runtime denies signing when simulation fails | 05-02 T3 (map_simulation_error factory) + 05-04 T2 (un-ignore stub) + 05-04 T3 (stdio revert grid) | `cargo test -p executor-mcp --lib errors::sim_factory_tests && cargo test -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_returns_simulation_failed_when_revert` | ❌ W0 |
| **EXE-05** | Runtime applies policy before signing | 05-03 T2 (evaluate function) + 05-04 T2 (orchestration STEP D — policy loop runs BEFORE STEP E sim loop per D-07) | `cargo test -p executor-policy --test eval_chains --test eval_contracts --test eval_selectors --test eval_native_value --test eval_erc20_spend --test eval_raw_calldata && cargo test -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_emits_decisions_when_actions_pass_policy_and_simulation` | ❌ W0 |
| **EXE-06** | Runtime denies signing when policy rejects an action | 05-03 T3 (map_policy_error factory) + 05-04 T2 (PolicyDenied transition + return -32017) + 05-04 T3 (6 stdio violation tests) | `cargo test -p executor-mcp --lib errors::policy_factory_tests && cargo test -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_returns_policy_violation_for_disallowed_chain strategy_run_returns_policy_violation_for_disallowed_target strategy_run_returns_policy_violation_for_disallowed_selector strategy_run_returns_policy_violation_for_value_cap strategy_run_returns_policy_violation_for_erc20_spend_cap strategy_run_returns_policy_violation_for_raw_calldata_when_not_allowed` | ❌ W0 |
| **POL-01** | Policy can restrict allowed chain IDs | 05-03 T2 (eval_chains.rs) + 05-04 T3 (strategy_run_returns_policy_violation_for_disallowed_chain) | `cargo test -p executor-policy --test eval_chains && cargo test -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_returns_policy_violation_for_disallowed_chain` | ❌ W0 |
| **POL-02** | Policy can restrict target contract addresses | 05-03 T2 (eval_contracts.rs) + 05-04 T3 (strategy_run_returns_policy_violation_for_disallowed_target) | `cargo test -p executor-policy --test eval_contracts && cargo test -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_returns_policy_violation_for_disallowed_target` | ❌ W0 |
| **POL-03** | Policy can restrict function selectors | 05-03 T2 (eval_selectors.rs) + 05-04 T3 (strategy_run_returns_policy_violation_for_disallowed_selector) | `cargo test -p executor-policy --test eval_selectors && cargo test -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_returns_policy_violation_for_disallowed_selector` | ❌ W0 |
| **POL-04** | Policy can restrict max native value per action | 05-03 T2 (eval_native_value.rs) + 05-04 T3 (strategy_run_returns_policy_violation_for_value_cap) | `cargo test -p executor-policy --test eval_native_value && cargo test -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_returns_policy_violation_for_value_cap` | ❌ W0 |
| **POL-05** | Policy can restrict max ERC20 spend for helper-generated ERC20 actions | 05-03 T2 (eval_erc20_spend.rs — D-16 cumulative semantics) + 05-04 T3 (strategy_run_returns_policy_violation_for_erc20_spend_cap) | `cargo test -p executor-policy --test eval_erc20_spend && cargo test -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_returns_policy_violation_for_erc20_spend_cap` | ❌ W0 |
| **POL-06** | Raw calldata actions are denied unless explicitly allowed by policy | 05-03 T2 (eval_raw_calldata.rs — deny-by-default) + 05-04 T3 (strategy_run_returns_policy_violation_for_raw_calldata_when_not_allowed) | `cargo test -p executor-policy --test eval_raw_calldata && cargo test -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_returns_policy_violation_for_raw_calldata_when_not_allowed` | ❌ W0 |
| **STJ-05** | Runtime records simulation results and policy decisions | 05-04 T1 (journal_decisions table + record_decision + MR-04 seq) + 05-04 T2 (orchestration writes per-gate rows) + 05-04 T3 (journal verification stdio tests) | `cargo test -p executor-state --test journal_decision_seq && cargo test -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_journal_records_pass_decisions_on_success strategy_run_journal_records_fail_decision_on_policy_denied strategy_run_records_skipped_simulation_when_policy_denied` | ❌ W0 |

**Coverage:** every Phase-5 requirement (EXE-01..06 + POL-01..06 + STJ-05) maps to ≥ 1 owning task with an automated test command. No requirement is unmapped.

**Carry-forward verification (D-13):** every plan's final task includes regression assertions against:
- HR-01 — `sandbox_blocks_host_globals` Phase-3 test (regression in EVERY plan).
- MR-01 — wire-detail grep tests in 05-01 T2 + 05-04 T3 (no raw alloy/serde/rusqlite substrings on wire).
- MR-03 — `record_decision` propagates `StateError::SerializationError` (05-04 T1).
- MR-04 — same-ms ordering test on `journal_decisions.seq` (05-04 T1).
- BR-01 — `data.kind == "policy_violation" | "simulation_failure"` (NOT "exception") in 05-04 T3.
- BR-02 — Action[] cap at `validate_strategy_output` (05-01 T3).
- WR-01 — `grep -c 'block_in_place' crates/executor-mcp/src/tools.rs` == 0 (05-04 T2 acceptance).
- WR-04 — `sanitize_revert_reason` consumed by simulate; revert text sanitized on wire (05-02 T2 anvil + 05-04 T3 stdio).
- D-12 — every Phase-5 status mutation routes through `update_run_status_with_transition` (05-04 T1 transition tests).
- D-15d — mutex hygiene during gate pipeline (05-04 T2 acceptance — implicit via successful concurrent test runs).

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies (12/12 tasks mapped).
- [x] Sampling continuity: no 3 consecutive tasks without automated verify.
- [x] Wave 0 covers all MISSING references mapped to owning tasks.
- [x] No watch-mode flags.
- [x] Feedback latency ≤ 60s (workspace ~15s default; ~60s with anvil).
- [x] `nyquist_compliant: true` set in frontmatter.
- [x] Every Phase-5 requirement (EXE-01..06 + POL-01..06 + STJ-05) has ≥ 1 task that exercises it via an automated command.
- [x] Phase-3 + Phase-4 carry-forward rules (HR-01, MR-01, MR-03, MR-04, BR-01, BR-02, WR-01, WR-04, D-12, D-15d) have explicit regression coverage in each plan.
- [x] Schema goldens for the 4 new types (Decision, PolicyVerdict, SimulationOutcome wire-mirror, PolicyConfig) + 2 regenerated (StrategyOutcome, StrategyRunResponse) tracked in Wave 0 and locked in Plan 05-04 Task 3.
- [x] BR-01 carry-forward test (`data.kind != "exception"`) is mapped to 05-04 T3 with explicit assertion.

**Approval:** approved 2026-04-27 (per-task map populated by planner; `wave_0_complete: true` will be set by executor once all Wave-0 fixture/source files land in their owning plan tasks and their automated commands exit 0).

---

## Notes for the Executor

1. **Anvil binary detection** — Plans 05-02 / 05-04 anvil-gated tests use the same `AnvilFixture::try_spawn` skip pattern as Phase 4 D-14. If `cargo test --workspace --features anvil-tests` reports a panic from the fixture, fix the fixture first — this is a wave-blocker for 05-02 / 05-04.

2. **The `cargo tree` audit for executor-policy alloy-FREE** (05-01 T1) is the durable D-20 guarantee. If `cargo update` ever pulls `alloy` into executor-policy's dep graph (e.g., via a transitive bump), the audit fails loudly.

3. **The schema-golden walker** for Plan 05-04 Task 3 reuses the Phase-3 round-trip pattern. Note that `executor-policy` schemars derives may need to be added (e.g., `PolicyConfig` adopts `JsonSchema` so the schema-for! macro resolves). If that change drags `schemars` into `executor-policy/Cargo.toml`, that's acceptable (still alloy-free). Schemars is a workspace-shared dep already.

4. **chain_id OnceCell does NOT cache errors** (Plan 05-04 T1) — `tokio::sync::OnceCell::get_or_try_init` re-tries on `Err`. If observed flaky against unstable RPCs, consider an `Arc<Mutex<Option<Result<u64, EvmError>>>>` upgrade. For v1 single-operator runtime, the simpler retry-on-miss path is correct.

5. **policy_get serialization** (Plan 05-03 T3) — adding `Serialize` derive to `LoadedPolicy` + sub-types is an additive change to Plan 05-01's model.rs. If any field doesn't trivially derive (e.g., `HashMap<(u64, Address), V>`), use `#[serde(serialize_with = ...)]` or convert to a wire-mirror struct at serialization time. The contract is "agents see a JSON view of the live policy" — exact field names CAN diverge from internal struct names (use `#[serde(rename = ...)]` if needed).

6. **revert_counter.hex** (Plan 05-02 Task 2) — must be COMPILED OUT-OF-BAND with `solc` or `forge build` before Plan 05-02 Task 2 can run. Document in 05-02 SUMMARY whether the bytecode was produced via `forge` or `solc` for audit reproducibility. Keep the bytecode small (~300 bytes is realistic for a 2-function revert contract).

7. **Workspace test count target after Phase 5:** ≥ 430 total. Phase 4 left 353. Phase 5 nets ~75 new tests across the four plans.

8. **Documentation updates** — every task's commit lands the test code; the docs (REQUIREMENTS.md traceability table marking EXE-01..06 + POL-01..06 + STJ-05 Complete, ROADMAP.md marking Phase 5 Complete) is the FINAL `docs(05)` commit after Plan 05-04 Task 3 lands. Mirror Phase-4 commit style (commit `84e3238` at HEAD).

9. **The `policy_violation` vs `policy_not_loaded` distinction** — both surface on `-32017` with different `data.kind`. `policy_violation` includes `data.rule` + `data.action_index`; `policy_not_loaded` includes neither. Agents dispatch on `data.kind` per BR-01. Tests in 05-04 T3 assert both wire shapes explicitly.

10. **Researcher conflict resolution recap** — the orchestrator brief locked TWO researcher-vs-PATTERNS resolutions in favor of the researcher; both are reflected in CONTEXT D-01 + D-08. Executor MUST NOT introduce -32019/-32020 wire codes (D-08) and MUST NOT promote `alloy` to `[workspace.dependencies]` (D-01 / D-20). Both are caught by the cargo tree audit + grep on errors.rs.
