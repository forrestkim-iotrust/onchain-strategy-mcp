---
phase: 05-simulation-and-policy-gate
fixed_at: 2026-04-28T00:00:00Z
review_path: .planning/phases/05-simulation-and-policy-gate/05-REVIEW.md
iteration: 1
findings_in_scope: 4
fixed: 4
skipped: 0
status: all_fixed
---

# Phase 05: Code Review Fix Report

**Fixed at:** 2026-04-28T00:00:00Z
**Source review:** .planning/phases/05-simulation-and-policy-gate/05-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 4
- Fixed: 4
- Skipped: 0

## Fixed Issues

### CR-01: Policy and simulation gates are fail-open when no policy is loaded

**Files modified:** `crates/executor-mcp/src/tools.rs`, `crates/executor-mcp/tests/stdio_handshake.rs`
**Commit:** d7d6fe2
**Applied fix:** `strategy_run` now rejects every non-`Noop` action when no policy is loaded, while preserving all-`Noop` arrays as gate-skippable. Added integration coverage for no-policy rejection of `raw_call`, `contract_call`, `erc20_transfer`, `erc20_approve`, and `native_transfer`.

### CR-02: PolicyDenied and SimulationDenied runs never receive `finished_at`

**Files modified:** `crates/executor-state/src/runs.rs`, `crates/executor-state/tests/run_lifecycle_transition.rs`
**Commit:** c8ce273
**Applied fix:** Added a shared terminal-status predicate including `SimulationDenied` and `PolicyDenied`, and used it when filling `finished_at`. Added assertions for `Running -> SimulationDenied` and `Running -> PolicyDenied`.

### CR-03: Phase 05 terminal denial statuses can be transitioned out of

**Files modified:** `crates/executor-state/src/runs.rs`, `crates/executor-state/tests/run_lifecycle_transition.rs`
**Commit:** c8ce273
**Applied fix:** The transition guard now rejects transitions out of all terminal states, including Phase 05 denial states. Added coverage for self-transitions and transitions back to `Running` from both denial statuses.

### CR-04: Normalization failures after `Running` leave runs stuck in `running`

**Files modified:** `crates/executor-mcp/src/tools.rs`, `crates/executor-mcp/tests/stdio_handshake.rs`
**Commit:** d7d6fe2
**Applied fix:** Normalization errors now record a runtime-error journal outcome and transition the run to `Failed` before returning the EVM error. Added an integration regression test using a loaded policy and malformed hand-built action.

---

_Fixed: 2026-04-28T00:00:00Z_
_Iteration: 1_
