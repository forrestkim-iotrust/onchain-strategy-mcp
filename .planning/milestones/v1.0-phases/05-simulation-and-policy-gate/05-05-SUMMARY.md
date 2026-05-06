---
phase: 05-simulation-and-policy-gate
plan: 05
subsystem: simulation-policy-journal
tags: [rust, mcp, stdio, anvil, policy, journal, evm-simulation]

requires:
  - phase: 05-simulation-and-policy-gate
    provides: policy evaluator, simulation adapter, journal_decisions table, strategy_run gate pipeline
provides:
  - anvil-backed stdio proof for simulation_failure wire mapping
  - policy-denial simulation skipped journal row
  - stdio journal assertions for policy/simulation pass, fail, and skipped decisions
  - stdio negative grid for all six policy violation rules
affects: [phase-05-verification, phase-06-local-managed-execution, phase-07-safety-tests]

tech-stack:
  added: [alloy node-bindings feature for executor-mcp anvil stdio tests]
  patterns:
    - policy denied actions record policy/fail plus simulation/skipped before returning
    - stdio policy tests assert JSON-RPC code and data.kind/data.rule taxonomy
    - anvil-backed stdio simulation failure tests deploy a minimal reverting fixture

key-files:
  created:
    - .planning/phases/05-simulation-and-policy-gate/05-05-SUMMARY.md
  modified:
    - crates/executor-mcp/src/tools.rs
    - crates/executor-mcp/tests/stdio_handshake.rs
    - crates/executor-mcp/Cargo.toml
    - crates/executor-evm/tests/fixtures/revert_counter.hex
    - Cargo.lock

key-decisions:
  - "Policy-denied actions journal a simulation/skipped row with stable detail before returning policy_violation, preserving policy-first short-circuiting."
  - "The stdio simulation failure proof uses raw_call against a deployed reverting bytecode fixture so policy can permit the action while simulation returns revert."
  - "Policy negative-grid tests run through strategy_run stdio and assert -32017/data.kind=policy_violation/data.rule for each POL-01..06 taxonomy string."

patterns-established:
  - "Journal resource assertions read journal://{run_id} instead of internal helper state."
  - "Policy fixture generation keeps the intended rule as the first failing evaluator dimension."

requirements-completed: [EXE-04, EXE-05, EXE-06, POL-01, POL-02, POL-03, POL-04, POL-05, POL-06, STJ-05]

duration: 15min
completed: 2026-04-28T12:55:06Z
---

# Phase 05 Plan 05: Gap Closure Summary

**Anvil-backed simulation_failure stdio proof plus durable policy/simulation decision journaling and six-rule policy_violation stdio coverage**

## Performance

- **Duration:** 15 min
- **Started:** 2026-04-28T12:40:25Z
- **Completed:** 2026-04-28T12:55:06Z
- **Tasks:** 3
- **Files modified:** 5 implementation/test files plus this summary

## Accomplishments

- Replaced the ignored `strategy_run_returns_simulation_failed_when_revert` stub with an anvil-backed stdio test that deploys a reverting fixture and asserts `-32017`, `data.kind="simulation_failure"`, `action_index=0`, and `fail_reason="revert"`.
- Added production journaling so policy-denied actions record a `simulation/skipped` row after the `policy/fail` row without invoking RPC simulation.
- Added stdio journal assertions for success (`policy/pass`, `simulation/pass`) and denial (`policy/fail`, `simulation/skipped`) through `journal://{run_id}`.
- Added stdio policy negative-grid tests for `chain_not_allowed`, `contract_not_allowed`, `selector_not_allowed`, `native_value_exceeds`, `erc20_spend_exceeds`, and `raw_call_denied`.

## Task Commits

1. **Task 1: Replace ignored simulation-failure stdio stub with anvil-backed proof** - `611942e` (test)
2. **Task 2: Record skipped simulation decision row on policy denial and assert journal rows** - `7c59ab8` (feat)
3. **Task 3: Add comprehensive stdio policy negative grid for six rule dimensions** - `1a29dd7` (test)

## Files Created/Modified

- `crates/executor-mcp/src/tools.rs` - Records `DecisionVerdict::Skipped` simulation decision rows on policy denial and writes stable skipped payloads.
- `crates/executor-mcp/tests/stdio_handshake.rs` - Adds simulation failure, journal decision, and policy negative-grid stdio coverage.
- `crates/executor-mcp/Cargo.toml` - Enables `alloy/node-bindings` for the anvil-gated stdio feature.
- `crates/executor-evm/tests/fixtures/revert_counter.hex` - Fixes the reverting bytecode fixture so it deploys cleanly under anvil.
- `Cargo.lock` - Updates dependency resolution for the executor-mcp anvil test feature.

## Decisions Made

- Kept policy-first short-circuiting: the skipped simulation row is an audit record only and does not call `simulate_one`.
- Used a raw call in the simulation failure stdio proof, allowing the permissive policy to admit the deployed reverting target while simulation produces the denial.
- Kept policy negative-grid assertions focused on MCP wire taxonomy rather than simulator behavior.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed malformed reverting bytecode fixture**
- **Found during:** Task 1
- **Issue:** The existing `revert_counter.hex` fixture had odd-length/malformed deployment bytecode and could not be deployed by the anvil-backed proof.
- **Fix:** Replaced it with minimal creation bytecode that deploys runtime code `60006000fd`, which always reverts.
- **Files modified:** `crates/executor-evm/tests/fixtures/revert_counter.hex`
- **Verification:** `cargo test -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_returns_simulation_failed_when_revert -- --nocapture`
- **Committed in:** `611942e`

**2. [Rule 3 - Blocking] Added anvil node-bindings feature for executor-mcp anvil tests**
- **Found during:** Task 1
- **Issue:** The stdio test crate needed to spawn/deploy against anvil directly, but executor-mcp's `anvil-tests` feature did not enable Alloy node bindings.
- **Fix:** Wired `anvil-tests = ["alloy/node-bindings"]` and added an optional alloy dependency for test-only anvil deployment.
- **Files modified:** `crates/executor-mcp/Cargo.toml`, `Cargo.lock`
- **Verification:** targeted anvil stdio test passed.
- **Committed in:** `611942e`

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both fixes were required to make the planned anvil-backed proof executable; no runtime scope was widened beyond skipped-simulation journaling.

## Issues Encountered

- GitNexus reported read-only FTS-index warnings during CLI calls, but impact and detect-change outputs still returned usable results.
- The exact multi-test cargo command form in the plan is not accepted by `cargo test`; equivalent filters were run individually and by substring.

## Verification

- `cargo test -p executor-mcp --test stdio_handshake strategy_run_records_skipped_simulation_when_policy_denied -- --nocapture` — passed.
- `cargo test -p executor-mcp --test stdio_handshake policy_violation -- --nocapture` — 6 passed.
- `cargo test -p executor-mcp --features anvil-tests --test stdio_handshake strategy_run_returns_simulation_failed_when_revert -- --nocapture` — passed.
- `cargo test --workspace` — 487 passed.
- `cargo clippy --workspace --all-targets -- -D warnings` — no issues.
- `gitnexus detect-changes --scope all` — no uncommitted code changes detected after task commits.

## Known Stubs

None found in files modified by this plan. Existing unimplemented `policy_update` remains intentional Phase 5/v2 behavior and is outside this gap plan.

## Threat Flags

None. The only production surface change is the planned audit-row addition for policy-denied actions; tests add coverage but no runtime endpoint or trust boundary.

## User Setup Required

None. Tests start anvil as needed when Foundry is installed; anvil was also ensured at `127.0.0.1:8545` for the stdio journal/policy tests.

## Next Phase Readiness

Phase 05 verification gaps are closed for EXE-04, EXE-05, EXE-06, POL-01..06, and STJ-05. Phase 6 can rely on `strategy_run` producing durable policy/simulation decisions before any signer work is introduced.

## Self-Check: PASSED

- Summary file exists at `.planning/phases/05-simulation-and-policy-gate/05-05-SUMMARY.md`.
- Task commits found: `611942e`, `7c59ab8`, `1a29dd7`.
- Key modified files exist: `crates/executor-mcp/src/tools.rs`, `crates/executor-mcp/tests/stdio_handshake.rs`, `crates/executor-mcp/Cargo.toml`, `crates/executor-evm/tests/fixtures/revert_counter.hex`, `Cargo.lock`.

---
*Phase: 05-simulation-and-policy-gate*
*Completed: 2026-04-28T12:55:06Z*
