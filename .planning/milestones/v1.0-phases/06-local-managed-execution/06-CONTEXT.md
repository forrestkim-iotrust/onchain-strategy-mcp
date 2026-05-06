# Phase 6: Local Managed Execution - Context

**Gathered:** 2026-04-28
**Status:** Ready for planning

<domain>
## Phase Boundary

Phase 6 turns Phase 5-approved transaction requests into real local managed execution: sign with a configured local EOA, broadcast to the configured RPC, wait for receipts, persist execution/receipt status, and expose that status to agents. It does not introduce external signer adapters, detached execution, scheduling, multi-account management, hosted custody, or dashboard/UI surfaces.

</domain>

<decisions>
## Implementation Decisions

### Local Signer Custody and Configuration
- The local private key is supplied indirectly through config by environment variable reference, e.g. `[signer].private_key_env = "EXECUTOR_PRIVATE_KEY"`; raw private keys must not be stored in committed config.
- Phase 6 supports one local EOA signer for v1, loaded at server boot or first execution boundary with fail-closed execution if absent/invalid.
- The resolved signer address may appear in execution reports/status output, but signer/private-key material must never be exposed to strategy JavaScript or journals.
- Broadcast requires explicit signer configuration; production config must not default to an anvil/dev private key. Tests may use anvil fixture keys.

### Broadcast and Receipt Semantics
- Approved actions execute sequentially in action order.
- The runtime waits for each receipt before broadcasting the next action so journals and reports preserve deterministic action-index ordering.
- Receipt waiting uses configured timeout defaults; timeout/stuck transactions produce a failed execution status without automatic replacement or retry.
- No automatic retry/replacement policy is included in Phase 6. Retry semantics are deferred until the core local runtime loop is proven.

### Execution Reporting and Status Surface
- `execution://{run_id}` and `execution_get` use persisted run/execution data as the status source of truth.
- Execution status includes run ID, strategy ID, signer address, per-action transaction hash, receipt status, gas used, execution error when present, and action index.
- Phase 6 should keep status/report shapes machine-readable and JSON-schema-backed like prior MCP tools/resources.
- Journal data remains the audit trail; execution report/status is the agent-facing summary of signed/broadcast/receipt outcomes.

### Claude's Discretion
- Exact internal module/function names, table names, and helper boundaries are at Claude's discretion as long as they preserve the existing crate boundaries and tests prove the success criteria.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `executor-signer` exists as an empty signer-boundary crate with `SignedTransaction` imported from `executor-core`.
- `executor-evm::normalize::NormalizedAction` already produces `TransactionRequest` values with `to`, `input`, and `value`; Phase 6 owns signer-side completion of gas/nonce/chain_id/from.
- `executor-mcp::tools::strategy_run` already validates, normalizes, policy-checks, simulates, and records decisions before success.
- `execution://{execution_id}` resource template exists but currently returns Phase 6 placeholder `resource_not_found`.
- State/journal patterns already support run lifecycle transitions, ordered journal rows, schema migrations through `SCHEMA_SQL`, and test-only deterministic helpers.

### Established Patterns
- Server boot prefers fail-closed behavior for missing security-critical config while preserving typed MCP errors at runtime.
- MCP tool errors use stable `-32017` runtime error shapes with `data.kind` and run IDs where available.
- Tests use stdio JSON-RPC integration coverage plus anvil-gated tests for live EVM behavior.
- Existing EVM calls use Alloy providers and anvil fixtures rather than introducing external services.

### Integration Points
- `strategy_run` is the execution pipeline insertion point after Phase 5 simulation/policy pass.
- `StateStore` and run/journal repositories are the persistence integration points for transaction hashes, receipts, gas, and errors.
- `resources.rs` must wire `execution://{run_id}` from placeholder to real execution report.
- `tools.rs` must ensure `execution_get` returns the same persisted status shape.

</code_context>

<specifics>
## Specific Ideas

- Keep Phase 6 local-hot-wallet wording explicit in docs and config examples.
- Prefer deterministic sequential action execution over concurrent broadcasting for v1 safety and traceability.
- Use anvil fixture keys only in tests; never introduce a production default private key.

</specifics>

<deferred>
## Deferred Ideas

- External signer adapters.
- Detached execution protocol.
- Multi-account/per-run signer selection.
- Retry/replacement policies for stuck transactions.
- Scheduler/reconcile loops.

</deferred>
