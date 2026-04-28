---
phase: 06-local-managed-execution
reviewed: 2026-04-28T00:00:00Z
depth: standard
files_reviewed: 22
files_reviewed_list:
  - crates/executor-core/src/schema/execution.rs
  - crates/executor-core/tests/schema_snapshots.rs
  - crates/executor-core/tests/schemas/ExecutionActionReport.json
  - crates/executor-core/tests/schemas/ExecutionGetResponse.json
  - crates/executor-core/tests/schemas/ExecutionIdInput.json
  - crates/executor-mcp/src/config.rs
  - crates/executor-mcp/src/resources.rs
  - crates/executor-mcp/src/server.rs
  - crates/executor-mcp/src/tools.rs
  - crates/executor-mcp/tests/execution_actions.rs
  - crates/executor-mcp/tests/stdio_handshake.rs
  - crates/executor-signer/Cargo.toml
  - crates/executor-signer/src/config.rs
  - crates/executor-signer/src/error.rs
  - crates/executor-signer/src/lib.rs
  - crates/executor-signer/src/local.rs
  - crates/executor-signer/tests/local_execution.rs
  - crates/executor-signer/tests/local_signer.rs
  - crates/executor-state/src/executions.rs
  - crates/executor-state/src/lib.rs
  - crates/executor-state/src/schema.rs
  - crates/executor-state/src/store.rs
  - crates/executor-state/tests/execution_actions.rs
findings:
  critical: 2
  warning: 1
  info: 0
  total: 3
status: issues_found
---

# Phase 06: Code Review Report

**Reviewed:** 2026-04-28T00:00:00Z
**Depth:** standard
**Files Reviewed:** 22
**Status:** issues_found

## Summary

Reviewed the Phase 6 local managed execution surface, focusing on state transitions, signing/broadcast flow, and execution receipt persistence. The implementation has correctness and safety defects that can strand runs in non-terminal states, broadcast actions that are not actually policy/simulation-approved, and lose signer attribution after a receipt timeout or failure.

## Critical Issues

### CR-01: Empty action arrays trigger signer failure and mark otherwise-successful runs failed

**File:** `crates/executor-mcp/src/tools.rs:342-345,576-590`

**Issue:** `approved_normalized` is initialized as an empty vector and is only populated inside the non-empty executable-action gate branch. For `strategy_run` results like `[]` or `[noop]`, `outcome` is still `StrategyOutcome::Actions`, so lines 576-590 call `execute_approved_actions` with an empty slice. Inside `execute_approved_actions`, `normalized.iter().all(Option::is_none)` is true for an empty slice, so it returns early if signer is configured; but before this call the handler reloads config and calls `self.chain_id()`. That means a no-action successful strategy can fail due to EVM chain-id or signer config problems even though there is nothing to sign or broadcast. This is incorrect behavior and a regression risk for no-op action arrays.

**Fix:** Only enter the signing path when there is at least one executable normalized action, not merely whenever the outcome is `Actions`.

```rust
if approved_normalized.iter().any(Option::is_some) {
    let signer_config = crate::config::load()
        .map_err(|e| storage_error(format!("load config for signer: {e}")))?
        .signer_config()
        .map_err(|e| storage_error(format!("parse signer config: {e}")))?;
    let chain_id = self.chain_id().await.map_err(|e| map_evm_error(e, &run_id))?;
    execute_approved_actions(
        &self.state,
        &run_id,
        self.evm_config.rpc_url.as_str(),
        signer_config.as_ref(),
        chain_id,
        &approved_normalized,
    )
    .await?;
}
```

### CR-02: Executable actions bypass signing after all-noop normalization branch

**File:** `crates/executor-mcp/src/tools.rs:381-385,576-590`

**Issue:** The signing decision is derived from `outcome` rather than from the approved executable action list. In the `normalized.iter().all(Option::is_none)` branch, `approved_normalized` remains empty while the outcome remains `Actions`. This currently causes signer/config checks for all-noop arrays (CR-01). More importantly, this coupling makes the broadcast boundary depend on a response-shape enum rather than the gate-approved transaction list, so future changes that return `Actions` with mixed or transformed data can easily sign the wrong set or skip signing. For transaction safety, signing and broadcast must be keyed only off the normalized actions that passed policy and simulation.

**Fix:** Treat `approved_normalized` as the single source of truth for managed execution. Populate it immediately after normalization, clear denied/skipped entries explicitly, and gate broadcast on `any(Option::is_some)`. Do not use `StrategyOutcome` shape to decide whether transactions exist.

```rust
// after policy/simulation have passed for executable actions
approved_normalized = normalized;

// later
if approved_normalized.iter().any(Option::is_some) {
    // load signer and broadcast exactly these approved actions
}
```

## Warnings

### WR-01: Receipt timeout/failure overwrites broadcast row with blank signer address

**File:** `crates/executor-state/src/executions.rs:116-120`

**Issue:** `record_execution_error` updates the row after receipt timeout/failure, but it does not preserve or explicitly include signer attribution in the insert fallback. If a broadcast row exists, signer address survives. If an error is recorded before `record_execution_broadcast` succeeds, the fallback inserts `signer_address = ''`. This makes `execution_get.signer_address` and per-action reports return an empty signer for signer/broadcast failures, which degrades execution reporting and makes audit trails ambiguous.

**Fix:** Accept an optional signer address at the error-recording boundary and persist it when available; for pre-signer failures, make the schema/report nullable instead of using an empty string sentinel.

```rust
pub(crate) fn record_execution_error(
    conn: &Connection,
    run_id: &str,
    action_index: i64,
    signer_address: Option<&str>,
    error_kind: &str,
    error_detail: Option<&str>,
) -> Result<(), StateError> {
    // update existing row as today; insert with signer_address.unwrap_or_default()
    // or migrate signer_address to nullable and insert NULL for no signer
}
```

---

_Reviewed: 2026-04-28T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
