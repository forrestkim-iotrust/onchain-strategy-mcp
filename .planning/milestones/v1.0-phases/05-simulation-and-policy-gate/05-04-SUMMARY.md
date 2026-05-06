---
phase: 05
plan: 04
subsystem: simulation-and-policy-gate
tags: [strategy_run, journal_decisions, policy_gate, simulation_gate, StrategyOutcome.decisions, phase5_emittable, chain_id_cache, anvil-tests, EXE-03, EXE-04, EXE-05, EXE-06, STJ-05]
status: complete
created: 2026-04-28
completed_date: 2026-04-28
dependency_graph:
  requires:
    - Plan 05-01 action normalization and validation cap
    - Plan 05-02 simulation adapter and sanitized simulation errors
    - Plan 05-03 policy loader, evaluator, and MCP policy error factories
  provides:
    - journal_decisions schema/repository surface and per-run seq ordering
    - strategy_run policy-before-simulation gate orchestration
    - StrategyOutcome::Actions.decisions response payload
    - policy and simulation decision journaling for STJ-05
    - Phase 5 run status terminal sinks for policy/simulation denial
  affects:
    - executor-core execution schemas and schema goldens
    - executor-state schema/run transition model
    - executor-mcp strategy_run pipeline and stdio tests
tech_stack:
  added: []
  patterns:
    - fail-closed policy gate before simulation/RPC execution
    - per-run journal_decisions sequence for deterministic ordering
    - policy and simulation GateVerdict values surfaced on success
    - direct git finalization without GSD commit helper attribution
key_files:
  created:
    - .planning/phases/05-simulation-and-policy-gate/05-04-SUMMARY.md
  modified:
    - crates/executor-core/src/schema/execution.rs
    - crates/executor-core/tests/schemas/StrategyOutcome.json
    - crates/executor-core/tests/schemas/StrategyRunResponse.json
    - crates/executor-mcp/src/tools.rs
    - crates/executor-mcp/tests/common/mod.rs
    - crates/executor-mcp/tests/stdio_handshake.rs
    - crates/executor-state/src/journal.rs
    - crates/executor-state/src/lib.rs
    - crates/executor-state/src/runs.rs
    - crates/executor-state/src/schema.rs
    - crates/executor-state/src/store.rs
    - crates/executor-state/tests/journal_decision_seq.rs
    - crates/executor-state/tests/journal_repo.rs
    - crates/executor-state/tests/run_base_model.rs
    - crates/executor-state/tests/run_lifecycle_transition.rs
    - crates/executor-evm/src/lib.rs
    - crates/executor-evm/src/native.rs
    - crates/executor-evm/src/simulate.rs
    - .planning/STATE.md
    - .planning/ROADMAP.md
    - .planning/REQUIREMENTS.md
decisions:
  - "Policy is evaluated before simulation; policy denial short-circuits before simulation rows for later actions."
  - "Noop and empty-action outcomes skip fail-closed policy/RPC gates because no transaction can reach signing."
  - "Missing policy transitions the run to Failed and returns policy_not_loaded before action normalization/simulation."
  - "Successful action outcomes include per-action policy and simulation GateVerdict::Pass entries."
metrics:
  task_count: 3
  files_created: 1
  files_modified: 20
  final_anvil_test: passed
  gitnexus_risk: critical
---

# Phase 05 Plan 04: Journaled Policy and Simulation Gate Summary

Phase 5 is complete. `strategy_run` now validates strategy output, normalizes actions, applies the loaded policy before any simulation, simulates approved transaction requests, records policy/simulation gate rows, and only then records a successful action outcome. Policy and simulation denials transition runs into terminal denial states and return stable MCP runtime errors before signing can exist in Phase 6.

## What Shipped

### Task 1 — journal_decisions, Phase-5 action emission, run transitions, and chain id cache

- Added the `journal_decisions` table to the SQLite schema with `run_id`, `action_index`, `gate`, `verdict`, optional `rule/detail`, serialized payload, `recorded_at`, and per-run `seq` protected by `UNIQUE(run_id, seq)`.
- Added the state-layer decision journal repository and façade surface in the prior 05-04 task commit: `DecisionGate`, `DecisionVerdict`, `DecisionEntry`, `record_decision`, `list_decisions_for_run`, and deterministic same-timestamp tests.
- Renamed the action journal emission predicate from `phase3_emittable` to `phase5_emittable` and widened it to include `SimulationFailure` and `PolicyDenied`.
- Extended run lifecycle handling so `Running -> SimulationDenied` and `Running -> PolicyDenied` are legal terminal transitions with `finished_at` populated, while Phase 6+ reserved states remain blocked.
- Added `StrategyOutcome::Actions { actions, decisions }` plus `ActionDecision` and `GateVerdict` schema types for the success path.
- Added lazy `ExecutorServer::chain_id()` caching via `OnceCell` in the prior task commit, so policy decisions use a single provider chain-id lookup per server instance.

**Commit:** `2e6f7c8` — `feat(05-04): journal_decisions repo + record_decision + phase5_emittable rename + chain_id OnceCell + StrategyOutcome.decisions`

### Task 2 — strategy_run gate orchestration

- Wired `tools::strategy_run` to run the Phase-5 gate pipeline after sandbox execution and output validation:
  1. skip gates for noop/empty action outputs;
  2. clone the loaded policy snapshot or fail closed with `policy_not_loaded`;
  3. normalize each non-noop action;
  4. evaluate policy with per-run ERC20 cumulative tally;
  5. record policy pass/fail decision rows;
  6. simulate policy-approved actions;
  7. record simulation pass/fail decision rows;
  8. populate `StrategyOutcome::Actions.decisions` on success.
- Policy denials record a `journal_decisions` fail row, record a `PolicyDenied` action outcome, transition `Running -> PolicyDenied`, and return `map_policy_error` with stable `data.kind = policy_violation` and the specific rule.
- Simulation failures record a simulation fail row, record a `SimulationFailure` action outcome, transition `Running -> SimulationDenied`, and return `map_simulation_error`.
- Storage locking remains scoped to state writes; no storage mutex is held while provider calls or simulation await points run.
- Failure semantics selected: policy denial short-circuits the pipeline; later actions do not receive simulation rows. This keeps denial causality clear and avoids implying simulation was attempted.

### Task 3 — stdio/anvil verification and policy config helpers

- Added stdio test helpers for spawning the server from arbitrary TOML and for creating permissive anvil policies.
- Updated anvil-gated strategy_run acceptance tests to use explicit policy configuration, preserving fail-closed behavior for default no-policy server startup.
- Verified the anvil-gated native transfer strategy path now passes with Foundry anvil running on `127.0.0.1:8545`.
- Regenerated StrategyOutcome/StrategyRunResponse schema goldens to include the new `decisions` surface.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Anvil was required for the final stdio simulation test**
- **Found during:** Final verification of `strategy_run_accepts_native_transfer`.
- **Issue:** The anvil-gated test could not run without a local JSON-RPC endpoint.
- **Fix:** User authorized installing/running Foundry anvil. Foundry was installed with `foundryup`, `anvil --host 127.0.0.1 --port 8545` was started, and the anvil-gated test passed.
- **Files modified:** None for the environment setup.
- **Commit:** final commit.

**2. [Rule 2 - Critical functionality] Existing acceptance tests needed explicit policies**
- **Found during:** Task 3 stdio wiring.
- **Issue:** Plan 05-03 intentionally made policy fail closed. Existing positive strategy_run tests using EVM actions would fail without policy configuration once Plan 05-04 enforced policy.
- **Fix:** Added test helpers for policy-backed server startup and updated anvil-gated acceptance paths to pass a permissive policy.
- **Files modified:** `crates/executor-mcp/tests/common/mod.rs`, `crates/executor-mcp/tests/stdio_handshake.rs`.
- **Commit:** final commit.

## Known Stubs

None. The plan does not leave placeholder UI data, mock data, TODO/FIXME markers, or unwired data sources in the modified files. Some schema-golden files are generated artifacts, not stubs.

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| threat_flag: trust-boundary | crates/executor-mcp/src/tools.rs | `strategy_run` now enforces policy and simulation at the MCP/runtime boundary before signing exists. |
| threat_flag: persistence | crates/executor-state/src/schema.rs | New `journal_decisions` table stores policy/simulation decision payloads and stable details. |

## Verification

| Check | Command | Result |
|-------|---------|--------|
| Initial status | `git status --short` | Only expected Plan 05-04 code/schema/test files modified before finalization. |
| Anvil-gated native transfer | `cargo test --manifest-path /Users/user/Documents/GitHub/onchain-strategy-mcp/.claude/worktrees/agent-acd3391d863cfd7af/Cargo.toml -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_accepts_native_transfer -- --nocapture` | passed: 1 passed, 58 filtered out (rerun during finalization). |
| GitNexus detect changes | `gitnexus detect-changes --repo onchain-strategy-mcp --scope all` | 10 files, 32 symbols, 20 affected processes, risk level: critical. |

## GitNexus Risk Summary

GitNexus reported **critical** risk for the full diff because the changes affect core execution flows: run insertion/status transitions, `strategy_run`, schema execution types, simulation helper usage, and state persistence paths. This is expected for a final Phase 5 gate integration plan because it touches the central execution pipeline and terminal-state transitions. No unexpected unrelated file families were reported.

## Self-Check: PASSED

- Summary file exists at `.planning/phases/05-simulation-and-policy-gate/05-04-SUMMARY.md`.
- Prior 05-04 task commit exists: `2e6f7c8`.
- Required tracking files were prepared for update: `.planning/STATE.md`, `.planning/ROADMAP.md`, `.planning/REQUIREMENTS.md`.
- Finalization uses direct `git commit` without GSD commit helper attribution.
- Final anvil-gated native transfer verification was rerun after summary/tracking updates and still passed.

## Remaining Blockers

None for Phase 05 Plan 04. Phase 6 remains responsible for signer custody, transaction broadcast, receipt watching, and execution status/reporting.