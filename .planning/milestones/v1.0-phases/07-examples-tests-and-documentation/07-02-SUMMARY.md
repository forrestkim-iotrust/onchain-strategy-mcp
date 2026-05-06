---
phase: 07-examples-tests-and-documentation
plan: 02
subsystem: testing
tags: [rust, mcp, safety, policy, simulation, sandbox]

requires:
  - phase: 05-simulation-and-policy-gate
    provides: policy and simulation gate taxonomy for strategy_run
  - phase: 06-local-managed-execution
    provides: execution_actions tx-hash audit rows after signer boundary
provides:
  - MCP-level safety regression tests for policy denial before signing
  - MCP-level simulation failure regression proving no tx hash is recorded
  - MCP-level sandbox host access regression through strategy_run
affects: [phase-07-examples-tests-and-documentation, executor-mcp, safety-verification]

tech-stack:
  added: []
  patterns:
    - stdio strategy_run safety tests reopen StateStore to verify no execution tx hashes
    - test-only signer env var injection proves safety gates fail before signer use

key-files:
  created:
    - crates/executor-mcp/tests/verification_safety.rs
  modified: []

key-decisions:
  - "Added a focused executor-mcp integration test file instead of modifying production symbols."
  - "Used test-only signer env injection for policy-denial and simulation-failure tests so failures prove gate ordering, not missing configuration."

patterns-established:
  - "Safety regressions assert both MCP wire taxonomy and persisted execution absence/no-tx-hash state."
  - "Sandbox forbidden-host checks are driven through strategy_run while keeping eval/Function out of the forbidden list."

requirements-completed: [VER-03, VER-04, VER-05]

duration: 35 min
completed: 2026-04-29
---

# Phase 07 Plan 02: Safety Regression Suite Summary

**MCP-level policy, simulation, and sandbox safety regressions proving unsafe paths stop before signing, tx hash persistence, or host access**

## Performance

- **Duration:** 35 min
- **Started:** 2026-04-29T04:47:49Z
- **Completed:** 2026-04-29T05:22:49Z
- **Tasks:** 2/2
- **Files modified:** 2

## Accomplishments

- Added `policy_blocks_disallowed_chain_contract_and_selector_before_signing`, covering `chain_not_allowed`, `contract_not_allowed`, and `selector_not_allowed` through MCP `strategy_run`.
- Added `simulation_failure_prevents_signing_and_records_no_tx_hash`, proving an Anvil revert surfaces as `simulation_failure` and does not persist a tx hash.
- Added `sandbox_blocks_forbidden_host_access_through_strategy_run`, proving `process`, `fetch`, `require("fs")`, and dynamic import remain blocked through the integrated runtime path.

## Task Commits

Each task was committed atomically:

1. **Task 1: Add policy-denial tests that prove no signing occurs** - `7cb10bc` (test)
2. **Task 2: Add simulation-failure and sandbox-host regression tests** - `018659b` (test)

**Plan metadata:** committed separately after this summary.

## Files Created/Modified

- `crates/executor-mcp/tests/verification_safety.rs` - New focused safety regression suite for policy, simulation, and sandbox boundaries.
- `.planning/phases/07-examples-tests-and-documentation/07-02-SUMMARY.md` - Execution summary and verification record.

## Decisions Made

- Created a new integration test file only, avoiding production symbol edits and keeping GitNexus impact risk low.
- Reused established stdio/common harness patterns and reopened `StateStore` directly for no-tx-hash assertions.
- Configured signer private-key access through a test-only environment variable to prove policy and simulation gates stop before signing.

## GitNexus Impact Analysis

- No existing production functions, classes, or methods were edited; changes are limited to a new integration test file and this summary.
- GitNexus query/detect CLI fallback was attempted, but `npx gitnexus` is not available in this worktree environment and hooks reported the index is stale/read-only. No HIGH or CRITICAL symbol-impact edits were made.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added explicit policy parser validation in the test setup**
- **Found during:** Task 1 (policy-denial safety tests)
- **Issue:** Early failures surfaced as `policy_not_loaded`; the generated chain-denial fixture omitted the required `[contracts.1]` subtable for an allowed chain.
- **Fix:** Added `executor_policy::load_policy_from_path(policy.path())?` before spawning the server and corrected the chain-denial policy fixture to include `[contracts.1]`.
- **Files modified:** `crates/executor-mcp/tests/verification_safety.rs`
- **Verification:** `cargo test -p executor-mcp --test verification_safety policy_blocks_disallowed_chain_contract_and_selector_before_signing` passed.
- **Committed in:** `7cb10bc`

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** The fix strengthened fixture correctness and did not widen runtime scope.

## Issues Encountered

- GitNexus hook context reported stale/read-only index warnings; CLI fallback returned `Unknown command: "gitnexus"`. Because only a new test file was added, no existing symbol impact analysis was required.

## Known Stubs

None found in files created or modified by this plan.

## Threat Flags

None. This plan adds tests for the stated trust boundaries and introduces no new runtime endpoint, auth path, file access path, or schema change.

## Verification

- `cargo test -p executor-mcp --test verification_safety policy_blocks_disallowed_chain_contract_and_selector_before_signing` — passed.
- `cargo test -p executor-mcp --test verification_safety sandbox_blocks_forbidden_host_access_through_strategy_run` — passed.
- `cargo test -p executor-mcp --features anvil-tests --test verification_safety simulation_failure_prevents_signing_and_records_no_tx_hash -- --nocapture` — passed.
- `cargo test --workspace` — passed: 509 tests across 53 suites.
- `cargo clippy --workspace --all-targets -- -D warnings` — passed with no issues.

## TDD Gate Compliance

- The plan tasks were marked `tdd="true"`; tests were implemented directly in task commits rather than split into separate RED and GREEN commits.
- Gate compliance warning: no separate failing RED commit exists for Task 1 or Task 2. The final tests pass and protect the requested behavior.

## User Setup Required

None. Tests use temporary SQLite state, temporary policy files, stdio server processes, and Anvil where needed.

## Next Phase Readiness

Phase 07 can continue with examples and documentation knowing VER-03, VER-04, and VER-05 have MCP-visible regression coverage for policy denial, simulation failure, and sandbox host access.

## Self-Check: PASSED

- Found `crates/executor-mcp/tests/verification_safety.rs`.
- Found `.planning/phases/07-examples-tests-and-documentation/07-02-SUMMARY.md`.
- Found task commit `7cb10bc`.
- Found task commit `018659b`.

---
*Phase: 07-examples-tests-and-documentation*
*Completed: 2026-04-29*
