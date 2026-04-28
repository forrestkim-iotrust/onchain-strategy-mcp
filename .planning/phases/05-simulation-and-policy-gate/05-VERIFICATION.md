---
phase: 05-simulation-and-policy-gate
verified: 2026-04-28T00:00:00Z
status: gaps_found
score: 11/13 must-haves verified
overrides_applied: 0
gaps:
  - truth: "EXE-04: Runtime denies signing when simulation fails is proven end-to-end at strategy_run"
    status: partial
    reason: "The simulation gate is implemented in tools::strategy_run, but the required anvil-gated stdio regression remains ignored with a panic stub, so the end-to-end MCP wire path for simulation_failure is not automatically verified."
    artifacts:
      - path: "crates/executor-mcp/tests/stdio_handshake.rs"
        issue: "strategy_run_returns_simulation_failed_when_revert still has #[ignore = \"enabled by Plan 05-04 — needs tools::strategy_run sim wiring\"] and panics instead of exercising strategy_run."
    missing:
      - "Remove #[ignore] from strategy_run_returns_simulation_failed_when_revert."
      - "Replace the panic stub with an anvil-backed strategy_run test that deploys the revert fixture, configures permissive policy, and asserts -32017 data.kind=simulation_failure/action_index=0/fail_reason=revert."
  - truth: "STJ-05: Runtime records simulation results and policy decisions, including skipped simulation decisions on policy denial"
    status: partial
    reason: "Policy pass/fail and simulation pass/fail rows are wired, but policy-denial short-circuit records only the policy fail row; no simulation skipped row is recorded, and the requested stdio journal coverage for policy-denied skipped simulation is absent."
    artifacts:
      - path: "crates/executor-mcp/src/tools.rs"
        issue: "Policy denial branch records DecisionGate::Policy fail and returns before recording DecisionGate::Simulation skipped."
      - path: "crates/executor-mcp/tests/stdio_handshake.rs"
        issue: "No stdio tests found for strategy_run_journal_records_pass_decisions_on_success, strategy_run_journal_records_fail_decision_on_policy_denied, or strategy_run_records_skipped_simulation_when_policy_denied."
    missing:
      - "Record a simulation skipped decision row when policy denies an action, or add an accepted override documenting that short-circuiting without skipped rows is the intended STJ-05 semantics."
      - "Add stdio journal assertions for success decisions, policy-denied fail decisions, and skipped-simulation semantics."
  - truth: "Plan 05-04 comprehensive stdio negative grid covers per-rule policy denials"
    status: failed
    reason: "The policy evaluator has per-dimension unit coverage, but the Plan 05-04 end-to-end stdio negative grid for chain/contract/selector/native_value/erc20_spend/raw_call policy violations is not present in stdio_handshake.rs."
    artifacts:
      - path: "crates/executor-mcp/tests/stdio_handshake.rs"
        issue: "grep found no stdio tests or assertions for chain_not_allowed, contract_not_allowed, selector_not_allowed, native_value_exceeds, erc20_spend_exceeds, or raw_call_denied."
    missing:
      - "Add strategy_run stdio tests that load policy fixtures/configs and assert each policy rule maps to -32017 data.kind=policy_violation with the expected data.rule."
human_verification:
  - test: "Live anvil strategy_run success path"
    expected: "With anvil running and a permissive policy configured, strategy_run_accepts_native_transfer passes and returns Actions with policy/simulation pass decisions."
    why_human: "The repository includes an anvil-gated path, but it depends on an external Foundry anvil process and local RPC availability."
---

# Phase 05: Simulation and Policy Gate Verification Report

**Phase Goal:** No transaction can reach the signer before simulation and policy approval.
**Verified:** 2026-04-28T00:00:00Z
**Status:** gaps_found
**Re-verification:** No — initial verification

## Goal Achievement

Phase 05 implements the core gate ordering in code: `strategy_run` validates output, normalizes actions, evaluates policy, simulates approved actions, records decision rows, and returns success only after both gates pass. There is no production signer call in Phase 05; signer work remains Phase 6. However, verification found required Phase 05 acceptance gaps in the end-to-end stdio test surface and in skipped-simulation journaling semantics.

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | EXE-01: Runtime validates `Action[]` before simulation or signing | VERIFIED | `validate_strategy_output` enforces `MAX_ACTIONS_PER_RUN` and deserializes/ABI-dry-runs actions before the Phase 5 gate pipeline in `crates/executor-mcp/src/tools.rs`. `cargo test -p executor-mcp --test stdio_handshake strategy_run_no_policy_rejects_raw_call` passed. |
| 2 | EXE-02: Runtime ABI-encodes contract call actions into transaction requests | VERIFIED | `executor_evm::normalize::normalize_action` and `encode_call_input` exist and are consumed by `strategy_run`; policy tests and normalize tests are present. |
| 3 | EXE-03: Runtime simulates transaction requests before signing | VERIFIED | `strategy_run` calls `simulate_one_latest(...)` after policy pass and before recording successful actions. No signer call exists in Phase 05 production code. |
| 4 | EXE-04: Runtime denies signing when simulation fails | PARTIAL | Code path transitions to `SimulationDenied` and returns `map_simulation_error`, but the required end-to-end stdio test remains ignored and panics. |
| 5 | EXE-05: Runtime applies policy before signing | VERIFIED | `strategy_run` snapshots policy, normalizes, builds `Decision`, and calls `evaluate(...)` before simulation. Missing policy fails closed with `policy_not_loaded`. |
| 6 | EXE-06: Runtime denies signing when policy rejects an action | VERIFIED | Policy denial branch records policy fail, records `PolicyDenied` action outcome, transitions to `PolicyDenied`, and returns `map_policy_error`. No signer path exists. |
| 7 | POL-01: Policy restricts allowed chain IDs | VERIFIED | `evaluate` emits `chain_not_allowed`; policy evaluator test suite passed. |
| 8 | POL-02: Policy restricts target contract addresses | VERIFIED | `evaluate` emits `contract_not_allowed`; policy evaluator test suite passed. |
| 9 | POL-03: Policy restricts function selectors | VERIFIED | `evaluate` emits `selector_not_allowed` for non-RawCall selectors; policy evaluator test suite passed. |
| 10 | POL-04: Policy restricts max native value per action | VERIFIED | `evaluate` emits `native_value_exceeds`; policy evaluator test suite passed. |
| 11 | POL-05: Policy restricts max ERC20 spend | VERIFIED | `evaluate` uses per-run tally and emits `erc20_spend_exceeds`; policy evaluator test suite passed. |
| 12 | POL-06: Raw calldata actions are denied unless explicitly allowed | VERIFIED | `evaluate` gates RawCall through `raw_call_allows` and emits `raw_call_denied`; policy evaluator test suite passed. |
| 13 | STJ-05: Runtime records simulation results and policy decisions | PARTIAL | `journal_decisions` table/repo exists and policy/simulation pass/fail rows are wired, but policy denial does not record a skipped simulation decision row and stdio journal coverage is missing. |

**Score:** 11/13 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|---|---|---|---|
| `crates/executor-mcp/src/tools.rs` | Gate pipeline policy -> simulation before success | PARTIAL | Core implementation exists; policy denial omits simulation skipped row. |
| `crates/executor-evm/src/simulate.rs` | `simulate_one`/`simulate_one_latest` adapter | VERIFIED | Async eth_call with timeout, typed pass/fail outcome, sanitized revert path. |
| `crates/executor-policy/src/eval.rs` | POL-01..06 evaluator | VERIFIED | Cheap-first short-circuit evaluator with stable rule taxonomy. |
| `crates/executor-state/src/schema.rs` | `journal_decisions` schema | VERIFIED | Table includes gate/verdict/rule/detail/payload/seq and run-id index. |
| `crates/executor-state/src/journal.rs` | `record_decision`/`list_decisions_for_run` | VERIFIED | Payload serialization propagates errors; list orders by recorded_at then seq. |
| `crates/executor-core/src/schema/execution.rs` | `StrategyOutcome::Actions.decisions` | VERIFIED | `ActionDecision` and `GateVerdict` schema types exist. |
| `crates/executor-mcp/tests/stdio_handshake.rs` | End-to-end negative grid and sim-failure test | FAILED | Simulation-failure test still ignored/panic stub; per-rule policy and journal stdio tests absent. |

### Key Link Verification

| From | To | Via | Status | Details |
|---|---|---|---|---|
| `strategy_run` | `normalize_action` | Step 7 normalization loop | VERIFIED | `normalize_action(action)` called before policy/sim. |
| `strategy_run` | `evaluate` | Policy loop | VERIFIED | `evaluate(&policy, &decision, &mut erc20_tally)` called before simulation. |
| `strategy_run` | `simulate_one_latest` | Simulation loop | VERIFIED | Called after all policy passes. |
| `strategy_run` | `StateStore::record_decision` | `record_decision_row` helper | PARTIAL | Pass/fail rows recorded; skipped simulation on policy denial missing. |
| `resources::read_journal` | `list_decisions_for_run` | journal resource body | VERIFIED | `decisions` array included in `journal://{run_id}` output. |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|---|---|---|---|---|
| `tools.rs::strategy_run` | `policy_snapshot` | `self.policy.read().await.clone()` loaded by `ExecutorServer::new_with_full_config` | Yes | VERIFIED |
| `tools.rs::strategy_run` | `normalized` | `normalize_action` over validated `Action[]` | Yes | VERIFIED |
| `tools.rs::strategy_run` | `decisions` | Policy and simulation loops mutate `ActionDecision` vector | Partial | Simulation skipped rows are not journaled on policy denial. |
| `resources.rs::read_journal` | `decisions` | `StateStore::list_decisions_for_run` | Yes | VERIFIED |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|---|---|---|---|
| Policy evaluator dimensions pass | `cargo test -p executor-policy --test eval_chains --test eval_contracts --test eval_selectors --test eval_native_value --test eval_erc20_spend --test eval_raw_calldata --test load_toml` | 46 passed | PASS |
| Decision journal and lifecycle pass | `cargo test -p executor-state --test journal_decision_seq --test run_lifecycle_transition` | 15 passed | PASS |
| No-policy non-noop fail-closed | `cargo test -p executor-mcp --test stdio_handshake strategy_run_no_policy_rejects_raw_call` | 1 passed | PASS |
| Simulation failure stdio test enabled | `grep` for ignored simulation test marker | Found `#[ignore = "enabled by Plan 05-04 — needs tools::strategy_run sim wiring"]` | FAIL |
| Per-rule stdio policy grid exists | `grep` for rule names in `stdio_handshake.rs` | No matches | FAIL |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|---|---|---|---|---|
| EXE-01 | 05-01 | Runtime validates `Action[]` before any simulation or signing. | SATISFIED | Output validation and cap occur before normalization/policy/sim. |
| EXE-02 | 05-01 | Runtime ABI-encodes contract call actions into transaction requests. | SATISFIED | `normalize_action` + `encode_call_input`; normalize tests. |
| EXE-03 | 05-02/05-04 | Runtime simulates transaction requests before signing. | SATISFIED | `simulate_one_latest` called before success; no signer exists. |
| EXE-04 | 05-02/05-04 | Runtime denies signing when simulation fails. | PARTIAL | Code path exists; end-to-end stdio test remains ignored. |
| EXE-05 | 05-04 | Runtime applies policy before signing. | SATISFIED | `evaluate` called before simulation/success. |
| EXE-06 | 05-04 | Runtime denies signing when policy rejects an action. | SATISFIED | Policy deny transitions `PolicyDenied` and returns policy error. |
| POL-01..06 | 05-03 | Six policy dimensions. | SATISFIED | Policy evaluator tests passed. |
| STJ-05 | 05-04 | Runtime records simulation results and policy decisions. | PARTIAL | Decision table/repo exists; skipped sim row and stdio journal tests missing. |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|---|---:|---|---|---|
| `crates/executor-mcp/tests/stdio_handshake.rs` | 2487-2514 | Ignored test with panic stub | Blocker | Required 05-04 simulation-failure end-to-end regression is not active. |
| `crates/executor-mcp/src/tools.rs` | 465-498 | Policy denial returns before skipped simulation row | Warning/Blocker for STJ-05 semantics | Journal omits requested skipped-simulation decision on policy denial. |

### Human Verification Required

1. Live anvil strategy_run success path

**Test:** Run the anvil-gated native-transfer strategy path against a live Foundry anvil endpoint and permissive policy.
**Expected:** `strategy_run` succeeds and returns `Actions` with policy and simulation pass decisions.
**Why human:** Requires an external local anvil service/RPC environment.

### Gaps Summary

The core safety implementation is present: policy is evaluated before simulation, simulation runs before success, denials transition to terminal states, and no signer call exists in Phase 05. The phase cannot be marked passed because required acceptance evidence and journaling semantics are incomplete: the simulation-failure stdio test remains an ignored panic stub, policy-denial skipped-simulation journaling is not implemented, and the comprehensive per-rule stdio negative grid requested by Plan 05-04 is absent.

---

_Verified: 2026-04-28T00:00:00Z_
_Verifier: gsd-verifier_
