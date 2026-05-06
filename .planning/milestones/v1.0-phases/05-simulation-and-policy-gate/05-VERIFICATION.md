---
phase: 05-simulation-and-policy-gate
verified: 2026-04-28T13:04:22Z
status: passed
score: 13/13 must-haves verified
overrides_applied: 0
re_verification:
  previous_status: gaps_found
  previous_score: 11/13
  gaps_closed:
    - "EXE-04 end-to-end strategy_run simulation failure proof is enabled and passes."
    - "STJ-05 policy denial now records policy fail plus simulation skipped rows and stdio journal assertions cover pass/fail/skipped decisions."
    - "Stdio strategy_run policy negative grid covers chain_not_allowed, contract_not_allowed, selector_not_allowed, native_value_exceeds, erc20_spend_exceeds, and raw_call_denied."
  gaps_remaining: []
  regressions: []
---

# Phase 05: Simulation and Policy Gate Verification Report

**Phase Goal:** No transaction can reach the signer before simulation and policy approval.
**Verified:** 2026-04-28T13:04:22Z
**Status:** passed
**Re-verification:** Yes — after gap closure plan 05-05

## Goal Achievement

Phase 05 now satisfies the roadmap contract. The `strategy_run` pipeline validates and normalizes returned actions, evaluates policy before simulation, simulates only policy-approved transaction requests, records policy/simulation gate decisions, and returns success only after both gates pass. Policy and simulation denials transition runs into terminal denial states and return stable MCP runtime errors. Phase 05 still contains no production signer path; Phase 6 introduces signing, and the Phase 05 gate is positioned before that future boundary.

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | EXE-01: Runtime validates `Action[]` before any simulation or signing | VERIFIED | `crates/executor-mcp/src/tools.rs` validates strategy output before the Phase 5 gate pipeline; `MAX_ACTIONS_PER_RUN` is enforced at the JSON-output gate. Existing stdio cap tests remain present. |
| 2 | EXE-02: Runtime ABI-encodes contract call actions into transaction requests | VERIFIED | `strategy_run` calls `normalize_action(action)` before policy/simulation; `executor_evm::normalize` builds `TransactionRequest` values with encoded calldata. |
| 3 | EXE-03: Runtime simulates transaction requests before signing | VERIFIED | `crates/executor-mcp/src/tools.rs` calls `simulate_one_latest(...)` after policy pass rows are recorded and before success response/action journaling. No signer call exists in Phase 05 production code. |
| 4 | EXE-04: Runtime denies signing when simulation fails | VERIFIED | Previous gap closed: `strategy_run_returns_simulation_failed_when_revert` has no `#[ignore]` or stub panic; it deploys reverting bytecode and asserts `-32017`, `data.kind="simulation_failure"`, `action_index=0`, and `fail_reason="revert"`. Ran `cargo test -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_returns_simulation_failed_when_revert -- --nocapture` — 1 passed. |
| 5 | EXE-05: Runtime applies policy before signing | VERIFIED | `strategy_run` snapshots loaded policy, builds `Decision` values from normalized actions, and calls `evaluate(&policy, &decision, &mut erc20_tally)` before any simulation call. |
| 6 | EXE-06: Runtime denies signing when policy rejects an action | VERIFIED | Policy denial branch records policy fail and simulation skipped rows, records `PolicyDenied`, transitions `Running -> PolicyDenied`, and returns `map_policy_error`. Ran `cargo test -p executor-mcp --test stdio_handshake policy_violation -- --nocapture` — 6 passed. |
| 7 | POL-01: Policy restricts allowed chain IDs | VERIFIED | `executor_policy::evaluate` emits `chain_not_allowed`; stdio test `strategy_run_returns_policy_violation_for_disallowed_chain` asserts `-32017/data.kind=policy_violation/data.rule=chain_not_allowed`. |
| 8 | POL-02: Policy restricts target contract addresses | VERIFIED | `evaluate` emits `contract_not_allowed`; stdio test `strategy_run_returns_policy_violation_for_disallowed_contract` asserts the wire rule. |
| 9 | POL-03: Policy restricts function selectors | VERIFIED | `evaluate` emits `selector_not_allowed` for non-RawCall selectors; stdio test `strategy_run_returns_policy_violation_for_disallowed_selector` asserts the wire rule. |
| 10 | POL-04: Policy restricts max native value per action | VERIFIED | `evaluate` emits `native_value_exceeds`; stdio test `strategy_run_returns_policy_violation_for_native_value_cap` asserts the wire rule. |
| 11 | POL-05: Policy restricts max ERC20 spend | VERIFIED | `evaluate` checks cumulative per-run ERC20 spend and emits `erc20_spend_exceeds`; stdio test `strategy_run_returns_policy_violation_for_erc20_spend_cap` asserts the wire rule. |
| 12 | POL-06: Raw calldata actions are denied unless explicitly allowed | VERIFIED | `evaluate` gates RawCall through `raw_call_allows` and emits `raw_call_denied`; stdio test `strategy_run_returns_policy_violation_for_raw_call_denied` asserts the wire rule. |
| 13 | STJ-05: Runtime records simulation results and policy decisions | VERIFIED | Previous gap closed: policy denial records `DecisionGate::Policy`/`Fail` and `DecisionGate::Simulation`/`Skipped` with detail `simulation skipped: policy denied action`; journal resource includes `decisions`. Ran skipped-simulation and success-journal stdio tests — both passed. |

**Score:** 13/13 truths verified

## Previously Missing Gaps Now Closed

| Previous Gap | Status | Evidence |
|---|---|---|
| EXE-04 end-to-end simulation failure proof was ignored/stubbed | CLOSED | `crates/executor-mcp/tests/stdio_handshake.rs` defines enabled `strategy_run_returns_simulation_failed_when_revert` at lines 2661-2715. `rg` found no `#[ignore]` for the test. Targeted anvil-feature test passed. |
| STJ-05 skipped simulation row and journal assertions were missing | CLOSED | `crates/executor-mcp/src/tools.rs` records `JournalDecisionVerdict::Skipped` for `DecisionGate::Simulation` before returning policy violation. Tests `strategy_run_journal_records_pass_decisions_on_success`, `strategy_run_journal_records_fail_decision_on_policy_denied`, and `strategy_run_records_skipped_simulation_when_policy_denied` exist and targeted checks passed. |
| Per-rule stdio policy negative grid was missing | CLOSED | `crates/executor-mcp/tests/stdio_handshake.rs` includes six `strategy_run_returns_policy_violation_for_*` tests covering every POL-01..06 rule. Ran `cargo test -p executor-mcp --test stdio_handshake policy_violation -- --nocapture` — 6 passed. |

## Required Artifacts

| Artifact | Expected | Status | Details |
|---|---|---|---|
| `crates/executor-mcp/src/tools.rs` | Gate pipeline policy -> simulation before success | VERIFIED | Normalization occurs before policy; policy pass/fail rows recorded; policy denial records simulation skipped; simulation pass/fail rows recorded; success only after simulation loop. |
| `crates/executor-evm/src/simulate.rs` | `simulate_one`/`simulate_one_latest` adapter | VERIFIED | Async `eth_call` with timeout, typed pass/fail outcome, sanitized revert path, and `simulate_one_latest` wrapper consumed by MCP runtime. |
| `crates/executor-policy/src/eval.rs` | POL-01..06 evaluator | VERIFIED | Cheap-first short-circuit evaluator emits stable rule taxonomy and mutates ERC20 tally only on allow. `cargo test -p executor-policy` — 59 passed. |
| `crates/executor-state/src/schema.rs` | `journal_decisions` schema | VERIFIED | Schema includes `journal_decisions` with gate/verdict/rule/detail/payload/recorded_at/seq and run-id index. |
| `crates/executor-state/src/journal.rs` | `record_decision`/`list_decisions_for_run` | VERIFIED | `record_decision` serializes payload with error propagation; `list_decisions_for_run` orders by `recorded_at ASC, seq ASC`. State tests passed. |
| `crates/executor-core/src/schema/execution.rs` | `StrategyOutcome::Actions.decisions` | VERIFIED | `ActionDecision` and `GateVerdict` schema types exist; `StrategyOutcome::Actions` includes defaulted `decisions`. |
| `crates/executor-mcp/tests/stdio_handshake.rs` | End-to-end sim-failure, journal, and policy negative-grid coverage | VERIFIED | Contains enabled simulation failure test, pass/fail/skipped journal tests, and six policy violation tests. Targeted tests passed. |

## Key Link Verification

| From | To | Via | Status | Details |
|---|---|---|---|---|
| `strategy_run` | `normalize_action` | Phase 5 gate pipeline | VERIFIED | `normalize_action(action)` is called for every returned action before policy/simulation. |
| `strategy_run` | `evaluate` | Policy loop | VERIFIED | `evaluate(&policy, &decision, &mut erc20_tally)` runs before `simulate_one_latest`. |
| `strategy_run` | `simulate_one_latest` | Simulation loop | VERIFIED | Simulation is invoked only after all policy checks for normalized actions pass. |
| `strategy_run` | `StateStore::record_decision` | `record_decision_row` helper | VERIFIED | Policy pass/fail, simulation pass/fail, and simulation skipped rows are all recorded. |
| `resources::read_journal` | `list_decisions_for_run` | journal resource body | VERIFIED | `journal://{run_id}` includes the `decisions` array populated from state. |
| `stdio_handshake` tests | MCP wire errors | `strategy_run` through JSON-RPC stdio | VERIFIED | Tests assert `-32017`, `data.kind`, and stable rule/failure fields through the actual MCP tool call path. |

## Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|---|---|---|---|---|
| `tools.rs::strategy_run` | `policy_snapshot` | `self.policy.read().await.clone()` loaded by server config | Yes | VERIFIED |
| `tools.rs::strategy_run` | `normalized` | `normalize_action` over validated `Action[]` | Yes | VERIFIED |
| `tools.rs::strategy_run` | `decisions` | Policy and simulation loops build `ActionDecision` and persist journal rows | Yes | VERIFIED |
| `resources.rs::read_journal` | `decisions` | `StateStore::list_decisions_for_run` | Yes | VERIFIED |
| `stdio_handshake.rs` | policy/simulation assertion data | Real `strategy_run` MCP calls plus `journal://{run_id}` resource reads | Yes | VERIFIED |

## Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|---|---|---|---|
| Policy-denied runs record simulation skipped | `cargo test -p executor-mcp --test stdio_handshake strategy_run_records_skipped_simulation_when_policy_denied -- --nocapture` | 1 passed | PASS |
| Six-rule stdio policy violation grid | `cargo test -p executor-mcp --test stdio_handshake policy_violation -- --nocapture` | 6 passed | PASS |
| Anvil-backed simulation failure MCP wire proof | `cargo test -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_returns_simulation_failed_when_revert -- --nocapture` | 1 passed | PASS |
| Success path journal decisions visible through resource | `cargo test -p executor-mcp --test stdio_handshake strategy_run_journal_records_pass_decisions_on_success -- --nocapture` | 1 passed | PASS |
| Decision journal and terminal lifecycle | `cargo test -p executor-state --test journal_decision_seq --test run_lifecycle_transition` | 15 passed | PASS |
| Policy evaluator unit/integration coverage | `cargo test -p executor-policy` | 59 passed | PASS |

## Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|---|---|---|---|---|
| EXE-01 | 05-01 | Runtime validates `Action[]` before any simulation or signing. | SATISFIED | Output validation and action cap occur before normalization/policy/simulation. |
| EXE-02 | 05-01 | Runtime ABI-encodes contract call actions into transaction requests. | SATISFIED | `normalize_action` + `encode_call_input`; consumed by `strategy_run`. |
| EXE-03 | 05-02/05-04 | Runtime simulates transaction requests before signing. | SATISFIED | `simulate_one_latest` called after policy pass and before success; no signer exists in Phase 05. |
| EXE-04 | 05-02/05-04/05-05 | Runtime denies signing when simulation fails. | SATISFIED | Enabled anvil-backed stdio test proves `simulation_failure`/revert wire response. |
| EXE-05 | 05-04/05-05 | Runtime applies policy before signing. | SATISFIED | Policy evaluator is called before simulation; missing policy fails closed. |
| EXE-06 | 05-04/05-05 | Runtime denies signing when policy rejects an action. | SATISFIED | Six stdio denial tests assert stable `policy_violation` rules. |
| POL-01 | 05-03/05-05 | Policy restricts allowed chain IDs. | SATISFIED | Unit coverage and stdio `chain_not_allowed` proof. |
| POL-02 | 05-03/05-05 | Policy restricts target contract addresses. | SATISFIED | Unit coverage and stdio `contract_not_allowed` proof. |
| POL-03 | 05-03/05-05 | Policy restricts function selectors. | SATISFIED | Unit coverage and stdio `selector_not_allowed` proof. |
| POL-04 | 05-03/05-05 | Policy restricts max native value per action. | SATISFIED | Unit coverage and stdio `native_value_exceeds` proof. |
| POL-05 | 05-03/05-05 | Policy restricts max ERC20 spend. | SATISFIED | Unit coverage and stdio `erc20_spend_exceeds` proof. |
| POL-06 | 05-03/05-05 | Raw calldata actions are denied unless explicitly allowed. | SATISFIED | Unit coverage and stdio `raw_call_denied` proof. |
| STJ-05 | 05-04/05-05 | Runtime records simulation results and policy decisions. | SATISFIED | `journal_decisions` is wired to `journal://`; pass/fail/skipped rows verified by stdio resource assertions. |

## Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|---|---:|---|---|---|
| `crates/executor-mcp/src/tools.rs` | 238 | Placeholder tools section comment | Info | Existing phase-gated placeholder section for future tool surfaces; not part of Phase 05 gate behavior. `policy_get` is implemented; `policy_update` remains intentionally unimplemented for v1 mutation scope. |
| `crates/executor-mcp/tests/stdio_handshake.rs` | various | Placeholder comments for Phase 1 prompts/resources | Info | Legacy test descriptions and assertions for intentionally placeholder prompts/resources; unrelated to Phase 05 simulation/policy gate. |

No blocker or warning anti-patterns were found in the Phase 05 gap-closure implementation. The previous ignored simulation test and missing skipped-simulation row are resolved.

## Human Verification Required

None. The previously human-dependent anvil path is now covered by automated tests that spawn or use anvil as needed, and the anvil-backed simulation-failure spot-check passed in this verification run.

## Gaps Summary

No remaining gaps. All roadmap success criteria and Phase 05 requirement IDs are satisfied against current code and tests.

---

_Verified: 2026-04-28T13:04:22Z_
