---
phase: 06-local-managed-execution
plan: 03
subsystem: execution
tags: [rust, mcp, sqlite, execution-report, resources]

requires:
  - phase: 06-local-managed-execution
    provides: [execution_actions persistence and local managed execution rows from 06-02]
provides:
  - JSON-schema-backed per-action ExecutionActionReport schema
  - execution_get reports built from persisted run and execution_actions rows
  - execution://{run_id} resource using the same persisted report builder as execution_get
affects: [phase-06-local-managed-execution, executor-core-schema, executor-mcp-tools, executor-mcp-resources]

tech-stack:
  added: []
  patterns:
    - shared MCP report builder for tool/resource status parity
    - execution resources validate ULID-shaped run IDs before state lookup
    - top-level report signer address derived from first persisted execution action row

key-files:
  created:
    - crates/executor-core/tests/schemas/ExecutionActionReport.json
  modified:
    - crates/executor-core/src/schema/execution.rs
    - crates/executor-core/tests/schema_snapshots.rs
    - crates/executor-core/tests/schemas/ExecutionGetResponse.json
    - crates/executor-core/tests/schemas/ExecutionIdInput.json
    - crates/executor-mcp/src/tools.rs
    - crates/executor-mcp/src/resources.rs
    - crates/executor-mcp/src/server.rs
    - crates/executor-mcp/tests/stdio_handshake.rs

key-decisions:
  - "execution_get and execution://{run_id} share build_execution_report to prevent status-shape drift."
  - "Resource malformed/unknown execution run IDs use resource_not_found envelopes, while execution_get preserves not_found tool semantics."
  - "ExecutionIdInput keeps the legacy execution_id field name but documents that callers pass a strategy_run run ID."

patterns-established:
  - "Build execution status from the run row plus ordered execution_actions rows; do not read journal rows for execution reports."
  - "Serialize ExecutionGetResponse directly for execution resources so schema and source of truth match the tool."

requirements-completed: [STJ-07, EXE-09]

duration: 8 min
completed: 2026-04-28
---

# Phase 06 Plan 03: Execution Status Surfaces Summary

**JSON-schema-backed receipt status reports exposed consistently through execution_get and execution://{run_id}**

## Performance

- **Duration:** 8 min
- **Started:** 2026-04-28T16:30:33Z
- **Completed:** 2026-04-28T16:38:33Z
- **Tasks:** 3/3
- **Files modified:** 8

## Accomplishments

- Added `ExecutionActionReport` and widened `ExecutionGetResponse` with optional top-level signer address plus defaulted per-action execution reports.
- Rebuilt `execution_get` around `build_execution_report`, which reads the run row and ordered `execution_actions` rows in one blocking state operation.
- Wired `execution://{run_id}` to the same report builder, with malformed run IDs returning stable `resource_not_found` / `malformed_id` envelopes.
- Updated server/resource descriptions to remove stale phase-gated wording and describe live receipt-backed execution reports.

## Task Commits

Each task was committed atomically:

1. **Task 1 RED: Add failing execution action schema snapshot** - `86c248e` (test)
2. **Task 1 GREEN: Widen execution report schema** - `9b6496a` (feat)
3. **Task 2 RED: Add failing execution_get report assertions** - `84aa5e9` (test)
4. **Task 2 GREEN: Return persisted execution reports** - `7fe4d05` (feat)
5. **Task 3 RED: Add failing execution resource parity test** - `a200c3e` (test)
6. **Task 3 GREEN: Wire execution resource reports** - `6a4d541` (feat)

**Plan metadata:** committed separately after this summary.

## Files Created/Modified

- `crates/executor-core/src/schema/execution.rs` - Adds `ExecutionActionReport`, widens `ExecutionGetResponse`, and documents `execution_id` as run ID input.
- `crates/executor-core/tests/schema_snapshots.rs` - Adds schema snapshot coverage for `ExecutionActionReport`.
- `crates/executor-core/tests/schemas/ExecutionActionReport.json` - New JSON schema golden for per-action execution reports.
- `crates/executor-core/tests/schemas/ExecutionGetResponse.json` - Updated response schema golden for signer address and action report fields.
- `crates/executor-core/tests/schemas/ExecutionIdInput.json` - Updated input schema description while preserving wire field name.
- `crates/executor-mcp/src/tools.rs` - Adds `build_execution_report` and maps execution rows into report actions.
- `crates/executor-mcp/src/resources.rs` - Routes `execution://{run_id}` through the shared report builder with run-ID validation.
- `crates/executor-mcp/src/server.rs` - Updates server instructions for live receipt-backed execution report surfaces.
- `crates/executor-mcp/tests/stdio_handshake.rs` - Adds assertions for no-row reports and tool/resource parity on seeded execution rows.

## Decisions Made

- Used one `build_execution_report` helper rather than separate tool/resource serializers to keep MCP surfaces identical.
- Converted unknown run IDs from the shared tool-style `not_found` error into resource-layer `resource_not_found` for `execution://` reads.
- Preserved the legacy `ExecutionIdInput.execution_id` wire field for compatibility while changing descriptions to run-ID semantics.

## GitNexus Impact Analysis

- `ExecutionGetResponse` upstream impact: LOW; one direct indexed caller, `execution_get`, and one affected process.
- `execution_get` upstream impact: LOW; no indexed upstream dependants.
- `read_resource_impl` upstream impact: LOW; one direct caller, `ExecutorServer::read_resource`, and one affected test module.
- `ExecutorServer::get_info` upstream impact: LOW; no indexed upstream dependants.
- `gitnexus detect-changes` was run before task commits and reported no indexed changes; hooks continued to warn about read-only FTS index writes in this worktree.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

- GitNexus FTS index maintenance repeatedly warned that write operations were not allowed in the worktree index. Required impact analysis and detect-changes still ran; `detect-changes` returned no indexed changes.
- `cargo test -p executor-mcp execution_resource` succeeded but matched zero tests. The specific resource behavior is covered by `cargo test -p executor-mcp --test stdio_handshake execution_status_surfaces_match`.

## Known Stubs

- `crates/executor-core/src/schema/execution.rs` still contains the pre-existing `SignedTransaction` placeholder comment for unrelated signed-transaction payload work. It is not part of the execution status report surface and does not block this plan.
- `crates/executor-mcp/src/tools.rs` module header still contains historical placeholder wording for earlier phase surfaces. The live `execution_get` handler is wired and the header text is not used as runtime instructions.
- `crates/executor-mcp/tests/stdio_handshake.rs` contains pre-existing Phase 7 prompt placeholder assertions and policy placeholder commentary. These are unrelated to the live execution status surfaces built here.

## Threat Flags

None beyond the plan threat model. This plan intentionally exposes persisted signer address, tx hash, receipt status, gas, and execution error fields through the MCP tool/resource boundary described by T-06-03-01 through T-06-03-04; no private-key material is serialized.

## Verification

- `cargo test -p executor-core execution` — passed, 3 matching tests.
- `cargo test -p executor-mcp execution_get` — passed, 1 matching test.
- `cargo test -p executor-mcp execution_resource` — command succeeded, 0 matching tests.
- `cargo test -p executor-mcp execution_actions` — passed, 1 matching test.
- `cargo test -p executor-mcp --test stdio_handshake execution_status_surfaces_match` — passed, 1 matching test.
- `cargo test --workspace` — passed, 507 tests across 52 suites.
- `cargo clippy --workspace --all-targets -- -D warnings` — passed with no issues.

## TDD Gate Compliance

- Task 1 RED and GREEN commits are present: `86c248e` before `9b6496a`.
- Task 2 RED and GREEN commits are present: `84aa5e9` before `7fe4d05`.
- Task 3 RED and GREEN commits are present: `a200c3e` before `6a4d541`.

## User Setup Required

None - no external service configuration required for the status-report surface. Non-noop execution still requires the signer configuration documented in 06-01 and 06-02.

## Next Phase Readiness

Phase 06 status surfaces are ready for phase verification: persisted execution rows can now be queried through both `execution_get` and `execution://{run_id}` with matching JSON. Phase 07 can consume this report shape when building agent-facing prompts or strategy authoring guidance.

## Self-Check: PASSED

- Found `.planning/phases/06-local-managed-execution/06-03-SUMMARY.md`.
- Found `crates/executor-core/src/schema/execution.rs`.
- Found `crates/executor-core/tests/schemas/ExecutionActionReport.json`.
- Found `crates/executor-mcp/src/tools.rs`.
- Found `crates/executor-mcp/src/resources.rs`.
- Found `crates/executor-mcp/src/server.rs`.
- Found `crates/executor-mcp/tests/stdio_handshake.rs`.
- Found task commit `86c248e`.
- Found task commit `9b6496a`.
- Found task commit `84aa5e9`.
- Found task commit `7fe4d05`.
- Found task commit `a200c3e`.
- Found task commit `6a4d541`.

---
*Phase: 06-local-managed-execution*
*Completed: 2026-04-28*
