# Phase 6: Local Managed Execution - Research

**Researched:** 2026-04-28  
**Domain:** Rust MCP runtime, Alloy 2.0 local signer, EVM broadcast/receipt persistence  
**Confidence:** HIGH for codebase integration and Alloy APIs; MEDIUM for final internal naming

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

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

### Deferred Ideas (OUT OF SCOPE)
- External signer adapters.
- Detached execution protocol.
- Multi-account/per-run signer selection.
- Retry/replacement policies for stuck transactions.
- Scheduler/reconcile loops.
</user_constraints>

## Summary

Phase 6 should extend the current synchronous `strategy_run` pipeline after Phase 5 policy and simulation gates, not create a detached executor. The current code already validates strategy output, normalizes actions into Alloy `TransactionRequest`, records policy/simulation decisions, and reaches success only after all gates pass; Phase 6 should insert a sequential local-managed execution loop before `record_action` and the final `Running -> Succeeded` transition. [VERIFIED: codebase `crates/executor-mcp/src/tools.rs` lines 246-625] [VERIFIED: `.planning/phases/05-simulation-and-policy-gate/05-VERIFICATION.md`]

Use Alloy 2.0.1's local signer stack directly: parse `alloy_signer_local::PrivateKeySigner` from the env-var value, configure it with the runtime chain id via `with_chain_id(Some(chain_id))`, attach it to `ProviderBuilder::new().wallet(pk).connect_http(url)`, then broadcast with `Provider::send_transaction(tx).await` and wait with `PendingTransactionBuilder::with_timeout(...).get_receipt().await`. [VERIFIED: cargo info `alloy`/`alloy-signer-local`] [VERIFIED: cargo registry source `alloy-signer-local-2.0.1/src/private_key.rs`] [VERIFIED: cargo registry source `alloy-provider-2.0.1/src/fillers/wallet.rs`] [CITED: https://docs.rs/alloy/2.0.1]

Persist execution attempts as first-class state rows keyed by run ID and `action_index`, separate from `journal_decisions`. The execution report/status surface should be built from these rows plus the `runs` row, so both `execution_get` and `execution://{run_id}` return the same JSON-schema-backed machine-readable shape. [VERIFIED: codebase `crates/executor-state/src/schema.rs`] [VERIFIED: codebase `crates/executor-mcp/src/resources.rs`] [VERIFIED: Phase 6 CONTEXT.md]

**Primary recommendation:** implement `executor-signer` as the Alloy-local-signer boundary, add a `journal_executions` or `execution_actions` table in `executor-state`, then call a sequential `execute_approved_actions` helper from `strategy_run` after simulation pass and before success journaling. [VERIFIED: codebase + cargo registry source]

## Project Constraints (from CLAUDE.md)

- Use GitNexus for code intelligence and navigation; the project is indexed as `onchain-strategy-mcp`. [VERIFIED: `CLAUDE.md`]
- Before editing any function, class, or method, run `gitnexus_impact`/CLI equivalent and report blast radius; warn before HIGH/CRITICAL edits. [VERIFIED: `CLAUDE.md`]
- Before committing, run `gitnexus_detect_changes()`/CLI equivalent. [VERIFIED: `CLAUDE.md`]
- Never rename symbols with find-and-replace; use GitNexus rename tooling. [VERIFIED: `CLAUDE.md`]
- GitNexus CLI queries in this research produced read-only FTS index warnings and no semantic results, so planners should not rely on graph-derived relationships until the GitNexus index write issue is resolved or `npx gitnexus analyze` is run in an environment that can update the index. [VERIFIED: GitNexus CLI output]

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| EXE-07 | Runtime signs approved transaction requests with a local signer. | Use `alloy_signer_local::PrivateKeySigner` parsed from env-var secret and provider `.wallet(pk)` filler after Phase 5 gates. [VERIFIED: cargo registry source] |
| EXE-08 | Runtime broadcasts signed transactions to configured RPC. | Use wallet-enabled Alloy provider and `Provider::send_transaction`; wallet filler signs locally then passes raw transaction to node. [VERIFIED: cargo registry source `wallet.rs`] |
| EXE-09 | Runtime waits for receipt and records confirmed/failed status. | Use `PendingTransactionBuilder::with_timeout(...).get_receipt().await`; persist hash/status/gas/error per action. [VERIFIED: cargo registry source `heart.rs`] |
| STJ-06 | Runtime records tx hash, receipt status, gas used, and execution errors. | Add state table/repository with per-run/per-action execution rows; include tx hash, status, gas used, and sanitized error. [VERIFIED: codebase schema patterns] |
| STJ-07 | Agent can query execution status by execution/run ID. | Widen `ExecutionGetResponse` and wire `execution://{run_id}` from same persisted report source. [VERIFIED: codebase `execution_get` and resource placeholder] |
</phase_requirements>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Private-key resolution | API / Backend | OS environment | Server config names the env var; runtime reads env and never exposes key to JS. [VERIFIED: Phase 6 CONTEXT.md] |
| Local signing | API / Backend | EVM provider wallet filler | `executor-signer` owns signer construction; Alloy wallet filler signs before raw broadcast. [VERIFIED: cargo registry source] |
| Broadcast and receipt wait | API / Backend | Configured RPC | Runtime submits to `[evm].rpc_url` and waits synchronously per action. [VERIFIED: Phase 6 CONTEXT.md] |
| Execution persistence | Database / Storage | API / Backend | SQLite is current source of truth for runs/journals; execution status must survive process restart. [VERIFIED: codebase `executor-state`] |
| MCP status surface | API / Backend | MCP resource layer | `execution_get` and `execution://` should serialize the same persisted report. [VERIFIED: codebase `tools.rs`/`resources.rs`] |
| Strategy JS | Browser / Client equivalent sandbox | — | Strategy JS must only produce actions; it must not receive signer material or direct provider access. [VERIFIED: REQUIREMENTS STR-04] |

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `alloy` | 2.0.1 | Provider, transaction request, wallet filler, pending transaction/receipt APIs. [VERIFIED: `cargo info alloy`] | Already used by `executor-evm`; version is current in crates.io search and lockfile. [VERIFIED: `cargo search alloy --limit 1`] |
| `alloy-signer-local` | 2.0.1 | Local private-key signer type `PrivateKeySigner`. [VERIFIED: `cargo info alloy-signer-local`] | Exposed by Alloy `signer-local` feature and directly provides env private-key parsing. [VERIFIED: cargo registry source] |
| `executor-signer` | workspace `0.1.0` | Project crate boundary for local signer config, signer errors, execution adapter. [VERIFIED: codebase `crates/executor-signer/src/lib.rs`] | Preserves existing crate boundary and prevents signer concerns from leaking into strategy-js. [VERIFIED: Phase 6 CONTEXT.md] |
| `executor-state` | workspace `0.1.0` | SQLite schema/repository for execution rows and status reports. [VERIFIED: codebase] | Existing state/journal patterns use typed façade methods over `StateStore`. [VERIFIED: codebase `store.rs`] |
| `executor-mcp` | workspace `0.1.0` | Orchestrates `strategy_run`, `execution_get`, and `execution://` resource. [VERIFIED: codebase] | It already owns Phase 5 gate orchestration and MCP wire mapping. [VERIFIED: codebase `tools.rs`] |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `tokio` | 1.x workspace | Async broadcast/receipt waits and timeout handling. [VERIFIED: root `Cargo.toml`] | Use for async Alloy provider calls; do not block inside `spawn_blocking` for network waits. [VERIFIED: existing `simulate_one_latest` call style] |
| `serde`/`schemars` | workspace | JSON-schema-backed MCP response shapes. [VERIFIED: root `Cargo.toml`] | Widen `ExecutionGetResponse` and new execution action rows. [VERIFIED: codebase schema patterns] |
| `hex` | 0.4 in executor-mcp | Encoding bytes/hashes for JSON payloads. [VERIFIED: `executor-mcp/Cargo.toml`] | Use for raw bytes only; transaction hashes and addresses can generally use `to_string()`. [VERIFIED: codebase helper usage] |
| `alloy-primitives` | 1.x | Address, U256, B256/TxHash primitives. [VERIFIED: crate Cargo.toml files] | Keep domain structs explicit and avoid lossy JS-number conversions. [VERIFIED: Phase 4/5 patterns in STATE.md] |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Alloy wallet-enabled provider | Manual transaction build/sign/RLP + `send_raw_transaction` | More control, but hand-rolls gas/nonce/chain-id filling and increases signing mistakes. Use provider wallet filler. [VERIFIED: cargo registry source `ProviderBuilder::new()` recommended fillers] |
| `send_transaction(...).get_receipt()` | `send_transaction_sync` | `send_transaction_sync` relies on `eth_sendTransactionSync` for builder input and may be less broadly supported; `get_receipt` uses normal pending transaction watcher semantics. [VERIFIED: cargo registry source `provider/trait.rs`] |
| Store execution data only in `journal_actions` payload | Dedicated execution table | Journal payload-only storage makes status queries brittle and hard to index by action; a table with `(run_id, action_index)` matches STJ-06. [VERIFIED: codebase schema patterns] |
| Add signer logic directly to `executor-mcp` | Keep in `executor-signer` | Direct wiring would violate the existing signer-boundary crate. [VERIFIED: codebase `executor-signer` scaffold] |

**Installation / dependency changes:**

```toml
# crates/executor-signer/Cargo.toml
alloy = { version = "2.0", default-features = false, features = [
  "provider-http",
  "rpc-types-eth",
  "reqwest-rustls-tls",
  "signer-local",
]
alloy-primitives = "1"
thiserror = { workspace = true }
tokio = { workspace = true }
```

[VERIFIED: `cargo info alloy` feature list] [VERIFIED: existing `executor-evm/Cargo.toml` feature pattern]

**Version verification:**

- `cargo info alloy` returned `version: 2.0.1`, docs `https://docs.rs/alloy/2.0.1`, rust-version `1.91`. [VERIFIED: cargo info]
- `cargo info alloy-signer-local` returned `version: 2.0.1`, docs `https://docs.rs/alloy-signer-local/2.0.1`, rust-version `1.91`. [VERIFIED: cargo info]
- `cargo search alloy --limit 1` and `cargo search alloy-signer-local --limit 1` returned `2.0.1`. [VERIFIED: crates.io search]

## Architecture Patterns

### System Architecture Diagram

```text
MCP tool strategy_run
  -> validate strategy id
  -> load strategy + insert run
  -> sandbox executes JS with ctx only
  -> validate Action[] / noop
  -> normalize Action -> TransactionRequest (to/input/value only)
  -> policy gate for every normalized action
     -> deny: journal policy fail + simulation skipped, terminal PolicyDenied
  -> simulation gate for every policy-approved action
     -> deny: journal simulation fail, terminal SimulationDenied
  -> Phase 6 execution loop (new)
     -> resolve local signer from [signer].private_key_env
     -> build wallet-enabled provider for configured RPC
     -> for action_index in original order:
          fill signer/from/chain/gas/nonce via Alloy provider fillers
          send_transaction(tx)
          persist tx_hash as broadcasted
          wait get_receipt() with configured timeout
          persist receipt_status/gas_used/error
          if failed or timeout: stop loop, terminal Failed
     -> all receipts success: record action outcome, terminal Succeeded
  -> execution_get / execution://{run_id}
     -> read runs row + execution rows
     -> return JSON report
```

[VERIFIED: codebase `strategy_run` flow] [VERIFIED: cargo registry Alloy provider APIs] [VERIFIED: Phase 6 CONTEXT.md]

### Recommended Project Structure

```text
crates/
├── executor-signer/
│   ├── src/lib.rs          # public signer boundary re-exports
│   ├── src/config.rs       # SignerConfig { private_key_env, receipt_timeout_ms? }
│   ├── src/error.rs        # SignerError with stable non-secret messages
│   └── src/local.rs        # PrivateKeySigner parsing + wallet provider execution
├── executor-state/src/
│   ├── schema.rs           # execution table DDL
│   ├── executions.rs       # record/list execution action rows
│   └── store.rs            # StateStore façade methods
├── executor-core/src/schema/
│   └── execution.rs        # ExecutionGetResponse widened with action reports
└── executor-mcp/src/
    ├── config.rs           # [signer] section, no private key field
    ├── server.rs           # signer config/handle on ExecutorServer
    ├── tools.rs            # strategy_run handoff + execution_get report
    └── resources.rs        # execution://{run_id} report
```

[VERIFIED: existing crate boundaries and files]

### Pattern 1: Local signer from env var, not TOML secret

**What:** Config stores only the env-var name; the value is read at execution boundary and parsed into `PrivateKeySigner`. [VERIFIED: Phase 6 CONTEXT.md]  
**When to use:** Always for v1 local managed execution. [VERIFIED: Phase 6 CONTEXT.md]  
**Example:**

```rust
// Source: alloy-signer-local 2.0.1 private_key.rs FromStr + lib.rs with_chain_id pattern
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::Signer;

let raw = std::env::var(private_key_env).map_err(|_| SignerError::MissingPrivateKeyEnv)?;
let signer: PrivateKeySigner = raw.parse().map_err(|_| SignerError::InvalidPrivateKey)?;
let signer = signer.with_chain_id(Some(chain_id));
let signer_address = signer.address();
```

[VERIFIED: cargo registry source `alloy-signer-local-2.0.1/src/private_key.rs` lines 223-229] [VERIFIED: cargo registry source `alloy-signer-local-2.0.1/src/lib.rs` lines 64-72]

### Pattern 2: Wallet-enabled provider owns fill/sign/broadcast

**What:** Use Alloy `ProviderBuilder::new().wallet(pk).connect_http(url)` so recommended fillers handle gas, nonce, and chain id while wallet filler signs locally. [VERIFIED: cargo registry source `builder.rs` and `wallet.rs`]  
**When to use:** For each execution run or cached signer/provider handle; v1 can build per execution to avoid stale secret state complexity. [ASSUMED]  
**Example:**

```rust
// Source: alloy-provider 2.0.1 fillers/wallet.rs example
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::TransactionRequest;
use alloy::signers::local::PrivateKeySigner;

let pk: PrivateKeySigner = private_key.parse()?;
let provider = ProviderBuilder::new().wallet(pk).connect_http(rpc_url);
let receipt = provider
    .send_transaction(TransactionRequest::default().to(to).value(value))
    .await?
    .with_timeout(Some(receipt_timeout))
    .get_receipt()
    .await?;
```

[VERIFIED: cargo registry source `alloy-provider-2.0.1/src/fillers/wallet.rs` lines 20-27 and 165-183] [VERIFIED: cargo registry source `alloy-provider-2.0.1/src/heart.rs` lines 183-225]

### Pattern 3: Persist broadcast before receipt wait

**What:** Insert/update an execution row as soon as `send_transaction` returns a `tx_hash`, then update the same row after receipt/timeout/failure. [VERIFIED: Alloy `PendingTransactionBuilder::tx_hash()` exists] [ASSUMED: exact table name]  
**When to use:** Every non-noop action, in action order. [VERIFIED: Phase 6 CONTEXT.md]  
**Why:** If receipt waiting times out or the process is interrupted after broadcast, the tx hash remains queryable and the report can show `broadcasted` or `failed_timeout`. [ASSUMED]

### Pattern 4: One persisted report builder for tool and resource

**What:** Implement a state-backed helper returning `ExecutionGetResponse`, then call it from both `execution_get` and `execution://{run_id}`. [VERIFIED: codebase currently duplicates read logic only for journal; recommendation]  
**When to use:** Phase 6 status surface. [VERIFIED: Phase 6 CONTEXT.md]  
**Why:** Keeps MCP tool/resource shapes consistent and makes schema tests straightforward. [VERIFIED: Phase 6 CONTEXT.md]

### Anti-Patterns to Avoid

- **Signing before all Phase 5 gates pass:** Phase 5's safety contract is “No transaction can reach the signer before simulation and policy approval.” [VERIFIED: ROADMAP.md and 05-VERIFICATION.md]
- **Putting raw private keys in config, logs, journal rows, test assertions, or MCP responses:** Only env-var names and signer address may surface. [VERIFIED: Phase 6 CONTEXT.md]
- **Defaulting production signer to anvil account 0:** Current simulation default uses anvil address for dev ergonomics, but broadcast requires explicit signer configuration. [VERIFIED: codebase `config.rs`; VERIFIED: Phase 6 CONTEXT.md]
- **Concurrent action broadcasts:** v1 must execute sequentially and wait for each receipt before the next action. [VERIFIED: Phase 6 CONTEXT.md]
- **Manual nonce management unless Alloy filler proves insufficient:** `ProviderBuilder::new()` includes recommended fillers for gas estimation, nonce management, and chain-id fetching. [VERIFIED: cargo registry source `builder.rs` lines 131-139]
- **Using `format!("{:?}", secret/signer)` in logs:** Alloy local signer's Debug omits private key, but secret strings can still leak if logged separately. [VERIFIED: cargo registry source `alloy-signer-local/src/lib.rs` lines 165-172]

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| ECDSA signing | Custom secp256k1 signing or RLP transaction code | `PrivateKeySigner` + Alloy wallet filler | Handles chain-id-aware transaction signing through Alloy traits. [VERIFIED: cargo registry source] |
| Gas/nonce/chain filling | Manual `eth_getTransactionCount`, gas price, gas limit fields in MCP | `ProviderBuilder::new()` recommended fillers | Recommended fillers handle gas estimation, nonce management, and chain-id fetching. [VERIFIED: cargo registry source `builder.rs`] |
| Receipt polling loop | Custom sleep/poll loop | `PendingTransactionBuilder::with_timeout(...).get_receipt()` | Alloy already watches pending tx and fetches receipt. [VERIFIED: cargo registry source `heart.rs`] |
| Execution report JSON assembly in multiple places | Separate tool/resource serializers | Shared response builder in MCP/state adapter | Prevents drift between `execution_get` and `execution://`. [VERIFIED: existing schema/resource patterns] |
| Secret redaction library | Bespoke masking in every caller | Do not include secret in error/log values; use typed `SignerError` variants | Absence is safer than redaction for private keys. [VERIFIED: Phase 6 CONTEXT.md] |

**Key insight:** Phase 6 is primarily an integration and persistence phase, not a cryptography or transaction-building phase; Alloy already provides the signer/provider machinery, while this project must enforce ordering, fail-closed configuration, auditability, and MCP status shape. [VERIFIED: cargo registry source + codebase]

## Common Pitfalls

### Pitfall 1: Signer config accidentally mirrors simulation defaults

**What goes wrong:** Runtime broadcasts with an anvil/dev private key because `[signer]` has an implicit default. [VERIFIED: Phase 6 CONTEXT.md]  
**Why it happens:** `[evm].simulation_from` currently defaults to anvil account 0 for eth_call ergonomics. [VERIFIED: codebase `config.rs`]  
**How to avoid:** Add `[signer].private_key_env: Option<String>` with no default; execution fails closed if absent. [VERIFIED: Phase 6 CONTEXT.md]  
**Warning signs:** Config tests expecting default `signer.private_key_env == Some(...)`; examples with committed raw private key. [ASSUMED]

### Pitfall 2: Returning success after broadcast but before receipt

**What goes wrong:** Agent sees `Succeeded` even though the chain later fails or reverts the transaction. [ASSUMED]  
**Why it happens:** Confusing “accepted into mempool” with “receipt-backed execution.” [ASSUMED]  
**How to avoid:** Only transition to `Succeeded` after all execution rows have successful receipts; failed receipt or timeout transitions run to `Failed`. [VERIFIED: Phase 6 success criteria]
**Warning signs:** Tests assert only `tx_hash` and not receipt status/gas. [ASSUMED]

### Pitfall 3: Losing tx hash on receipt timeout

**What goes wrong:** Broadcast succeeds, timeout fires, but status report cannot show which tx is pending/stuck. [ASSUMED]  
**Why it happens:** Code awaits receipt before persisting anything. [ASSUMED]  
**How to avoid:** Persist `tx_hash` immediately after `send_transaction` returns and update row status after receipt wait. [VERIFIED: Alloy `PendingTransactionBuilder::tx_hash()` exists]
**Warning signs:** Execution table insert happens only after `get_receipt()`. [ASSUMED]

### Pitfall 4: Action index drift after filtering noops

**What goes wrong:** Status row action indices refer to filtered normalized vector positions, not original strategy action positions. [VERIFIED: current normalized vector contains `Option<NormalizedAction>` preserving positions in `tools.rs`]  
**Why it happens:** Noop actions normalize to `None`; naive compaction changes indices. [VERIFIED: codebase `tools.rs` lines 383-405]
**How to avoid:** Keep original `enumerate()` index and persist `action_index` from original action array. [VERIFIED: Phase 6 CONTEXT.md]
**Warning signs:** `action_index` equals execution loop counter over `Vec<NormalizedAction>` after filtering. [ASSUMED]

### Pitfall 5: Leaking raw RPC/alloy errors to MCP wire

**What goes wrong:** Error body includes raw transport, node, or transaction details that may be unstable or sensitive. [VERIFIED: existing MR-01 discipline in `errors.rs`]  
**Why it happens:** Using `e.to_string()` directly in `ExecutionAction.error`. [ASSUMED]
**How to avoid:** Define stable execution error taxonomy: `signer_not_configured`, `invalid_private_key`, `broadcast_failed`, `receipt_timeout`, `receipt_failed`, `receipt_missing`. Log raw details with tracing only. [VERIFIED: existing error mapping pattern]
**Warning signs:** Tests asserting substrings like `Reqwest`, `TransportError`, private key, or full TOML parse text. [ASSUMED]

### Pitfall 6: Holding `StateStore` mutex across network await

**What goes wrong:** Runtime serializes or deadlocks DB access while waiting for RPC receipts. [VERIFIED: codebase `StateStore` doc warns not to hold mutex across await]  
**Why it happens:** Updating execution row and awaiting provider in one locked block. [ASSUMED]
**How to avoid:** Use short `spawn_blocking` DB calls before/after async network operations, mirroring current MCP patterns. [VERIFIED: codebase `tools.rs`]
**Warning signs:** `let mut store = state.lock().await; provider.send_transaction(...).await`. [ASSUMED]

## Code Examples

### Signer boundary shape

```rust
// Source: project executor-signer scaffold + alloy-signer-local 2.0.1
pub struct LocalSignerConfig {
    pub private_key_env: String,
    pub receipt_timeout: std::time::Duration,
}

pub struct LocalSignerHandle {
    signer: alloy::signers::local::PrivateKeySigner,
    signer_address: alloy_primitives::Address,
}
```

[VERIFIED: codebase `executor-signer` scaffold] [VERIFIED: cargo registry source] [ASSUMED: exact struct names]

### Execution row schema shape

```sql
-- Source: follows existing executor-state schema.rs append-only/journal style
CREATE TABLE IF NOT EXISTS execution_actions (
    id              TEXT PRIMARY KEY,
    run_id          TEXT NOT NULL REFERENCES runs(id),
    action_index    INTEGER NOT NULL,
    signer_address  TEXT NOT NULL,
    tx_hash         TEXT,
    status          TEXT NOT NULL,
    receipt_status  TEXT,
    gas_used        TEXT,
    error_kind      TEXT,
    error_detail    TEXT,
    recorded_at     TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    UNIQUE (run_id, action_index)
);
CREATE INDEX IF NOT EXISTS idx_execution_actions_run_id
    ON execution_actions(run_id);
```

[VERIFIED: existing SQLite schema conventions] [ASSUMED: exact table/column names]

### Sequential execution loop pseudocode

```rust
// Source: Phase 6 CONTEXT ordering + Alloy provider APIs
for (idx, normalized_action) in normalized.iter().enumerate() {
    let Some(na) = normalized_action else { continue };

    let pending = provider.send_transaction(na.tx.clone()).await?;
    let tx_hash = *pending.tx_hash();
    record_broadcast(run_id, idx, signer_address, tx_hash).await?;

    let receipt = pending
        .with_timeout(Some(receipt_timeout))
        .get_receipt()
        .await?;
    record_receipt(run_id, idx, &receipt).await?;
}
```

[VERIFIED: cargo registry source `Provider::send_transaction`, `PendingTransactionBuilder::tx_hash`, `with_timeout`, `get_receipt`] [ASSUMED: project helper names]

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Manual signer trait placeholder | Alloy local signer + wallet filler | Phase 6 implementation target | Replace empty `Signer` trait with real env-local signer boundary. [VERIFIED: codebase scaffold] |
| Phase 5 only simulates and records gates | Phase 6 signs/broadcasts only after gates pass | Phase 6 | `strategy_run` success becomes receipt-backed, not just gate-backed. [VERIFIED: roadmap] |
| `execution_get` returns base run row only | `execution_get` returns run plus per-action execution report | Phase 6 | STJ-07 becomes useful for agents. [VERIFIED: current `ExecutionGetResponse`; Phase 6 CONTEXT.md] |
| `execution://` placeholder not_found | `execution://{run_id}` reads persisted execution report | Phase 6 | Resource and tool status surfaces converge. [VERIFIED: current `resources.rs`] |

**Deprecated/outdated:**
- Empty `executor-signer::Signer` trait is a Phase 1 scaffold and should be replaced or widened in Phase 6. [VERIFIED: codebase `executor-signer/src/lib.rs`]
- Using `execution_id` wording while the actual identifier is a run ID is legacy wording; keep wire compatibility but document that it accepts `run_id`. [VERIFIED: codebase `ExecutionIdInput`] [ASSUMED: whether to rename schema field]

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Building a wallet-enabled provider per execution is acceptable for v1. | Pattern 2 | If too slow, planner may need cached signer/provider cell and extra lifecycle tests. |
| A2 | Dedicated `execution_actions` table is the best exact table shape. | Code Examples | If project prefers journal-only storage, plan must adapt persistence design. |
| A3 | Execution error taxonomy should include `signer_not_configured`, `invalid_private_key`, `broadcast_failed`, `receipt_timeout`, `receipt_failed`, `receipt_missing`. | Pitfall 5 | Wire tests may need a different taxonomy if user has hidden preferences. |
| A4 | `execution_id` should remain accepted as field name while semantically being `run_id`. | State of the Art | Renaming may break existing schema golden expectations; planner should inspect tests. |

## Open Questions

1. **Should receipt timeout live in `[signer]` or `[evm]`?**
   - What we know: `[evm].call_timeout_ms` is used for read/simulation calls; Phase 6 context says receipt waiting uses configured timeout defaults. [VERIFIED: codebase config + CONTEXT]
   - What's unclear: Whether to add `[signer].receipt_timeout_ms` or `[evm].receipt_timeout_ms`. [ASSUMED]
   - Recommendation: Add `[signer].receipt_timeout_ms` or `[execution].receipt_timeout_ms` only if introducing an execution config section; avoid reusing call timeout because receipt waits are longer than eth_call timeouts. [ASSUMED]

2. **Should failed receipt stop remaining actions?**
   - What we know: Actions execute sequentially and wait for each receipt before broadcasting the next action. [VERIFIED: Phase 6 CONTEXT.md]
   - What's unclear: CONTEXT does not explicitly state whether a reverted/failed receipt halts subsequent actions. [VERIFIED: CONTEXT.md]
   - Recommendation: Halt on first non-success receipt and mark run `Failed`; broadcasting later dependent actions after a failed earlier action is unsafe. [ASSUMED]

3. **Which receipt status field should be serialized?**
   - What we know: Requirement asks for receipt status and gas used. [VERIFIED: REQUIREMENTS.md]
   - What's unclear: Exact Alloy receipt accessor/field naming should be confirmed during implementation against compiler because receipt response type is network-associated. [VERIFIED: cargo registry source uses `receipt.transaction_hash` and `receipt.from` in tests]
   - Recommendation: Persist a project enum/string `confirmed`/`failed` derived from the receipt status and gas as decimal string. [ASSUMED]

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|-------------|-----------|---------|----------|
| Rust/Cargo | Build/test workspace | ✓ | cargo 1.94.0 | None needed. [VERIFIED: environment probe] |
| anvil | Anvil-backed signing/broadcast/receipt tests | ✓ | 1.5.1-stable | Gate with `--features anvil-tests` if unavailable in CI. [VERIFIED: environment probe] |
| Node/npm | ctx7/GitNexus CLI support | ✓ | node v25.5.0, npm 11.8.0 | Not required for Rust test execution. [VERIFIED: environment probe] |
| gitnexus | Required by project navigation rules | ✓ | 1.6.3 | CLI currently warns about read-only FTS index; use direct file reads if queries return empty. [VERIFIED: environment probe + query output] |
| ctx7 CLI | Preferred docs lookup fallback | ✗ | `npx --yes ctx7...` failed in npm | Use cargo info, docs.rs, and local cargo registry source. [VERIFIED: failed command] |

**Missing dependencies with no fallback:**
- None for Phase 6 implementation/test on this machine. [VERIFIED: environment probe]

**Missing dependencies with fallback:**
- ctx7 fallback unavailable; use docs.rs/cargo registry source for Alloy API verification. [VERIFIED: failed command]

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust `cargo test` with unit/integration tests; anvil tests behind crate features. [VERIFIED: codebase] |
| Config file | Cargo workspace `Cargo.toml`; no separate test config. [VERIFIED: codebase] |
| Quick run command | `cargo test -p executor-signer -p executor-state -p executor-mcp execution` [ASSUMED: filters after tests are added] |
| Full suite command | `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` [VERIFIED: Phase 05 summary] |
| Anvil run command | `cargo test -p executor-mcp --features anvil-tests --test stdio_handshake local_managed_execution -- --nocapture` [ASSUMED: exact test filter] |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|--------------|
| EXE-07 | Missing signer fails closed and valid env key derives signer address without exposing key. | unit + stdio integration | `cargo test -p executor-signer; cargo test -p executor-mcp signer_not_configured` | ❌ Wave 0 |
| EXE-08 | Approved native transfer broadcasts to configured anvil RPC and persists tx hash. | anvil integration | `cargo test -p executor-mcp --features anvil-tests --test stdio_handshake local_managed_execution_broadcasts` | ❌ Wave 0 |
| EXE-09 | Runtime waits for receipt and records confirmed/failed status + gas used. | anvil integration | `cargo test -p executor-mcp --features anvil-tests --test stdio_handshake local_managed_execution_records_receipt` | ❌ Wave 0 |
| STJ-06 | State repository round-trips execution rows by run/action order. | state integration | `cargo test -p executor-state execution_actions_roundtrip` | ❌ Wave 0 |
| STJ-07 | `execution_get` and `execution://{run_id}` return same persisted status shape. | stdio/resource integration | `cargo test -p executor-mcp --test stdio_handshake execution_status_surfaces_match` | ❌ Wave 0 |

### Sampling Rate

- **Per task commit:** `cargo test -p <changed-crate>` plus targeted stdio/state test. [ASSUMED]
- **Per wave merge:** `cargo test --workspace`. [VERIFIED: project pattern]
- **Phase gate:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and anvil feature tests for Phase 6 broadcast path. [VERIFIED: Phase 05 verification pattern]

### Wave 0 Gaps

- [ ] `crates/executor-signer/src/{config,error,local}.rs` tests for env var missing/invalid/no debug key leak. [ASSUMED]
- [ ] `crates/executor-state/tests/execution_actions.rs` for schema/repository ordering and update semantics. [ASSUMED]
- [ ] `crates/executor-mcp/tests/stdio_handshake.rs` Phase 6 cases for missing signer, broadcast/receipt, and execution resource. [VERIFIED: existing test file pattern]
- [ ] Config parser tests for `[signer]` unknown fields and no default private key env. [VERIFIED: existing `config.rs` test style]

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | MCP stdio local runtime has no user auth surface in v1. [VERIFIED: roadmap scope] |
| V3 Session Management | no | No session/cookie lifecycle in stdio transport. [VERIFIED: roadmap scope] |
| V4 Access Control | yes | Policy gate must run before signer handoff; no signing on policy/simulation denial. [VERIFIED: Phase 05 verification] |
| V5 Input Validation | yes | Existing action/config validation plus signer env-var validation. [VERIFIED: codebase validation patterns] |
| V6 Cryptography | yes | Use Alloy `PrivateKeySigner`; never implement cryptography manually. [VERIFIED: cargo registry source] |
| V7 Error Handling and Logging | yes | Stable MCP error taxonomy; raw details to tracing only; no key logging. [VERIFIED: existing `errors.rs` patterns] |
| V10 Malicious Code | yes | Strategy JS remains sandboxed and cannot access private keys. [VERIFIED: REQUIREMENTS STR-04] |

### Known Threat Patterns for Rust + local EVM signer

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Private key disclosure through config/log/journal/MCP response | Information Disclosure | Store only env-var name in config; never serialize raw env value; typed errors omit secret. [VERIFIED: Phase 6 CONTEXT.md] |
| Signing unapproved action | Elevation of Privilege/Tampering | Execute signer loop only after policy and simulation pass for all normalized actions. [VERIFIED: Phase 05 verification] |
| Replay or wrong-chain signing | Tampering | Set signer chain id from runtime `chain_id()`/configured RPC; include chain id in policy gate. [VERIFIED: codebase `chain_id()` and policy decision] |
| Non-deterministic multi-action execution | Tampering/Repudiation | Sequential action order; wait receipt before next broadcast; persist `action_index`. [VERIFIED: Phase 6 CONTEXT.md] |
| RPC/provider raw error leakage | Information Disclosure | Map broadcast/receipt errors to stable execution error kinds; raw detail only in tracing. [VERIFIED: existing MR-01 error mapping discipline] |
| Hot wallet misuse | Spoofing/Elevation of Privilege | Explicit signer config required; docs/tests label v1 as local hot-wallet runtime. [VERIFIED: Phase 6 CONTEXT.md] |

## Sources

### Primary (HIGH confidence)

- Project files read: `.planning/phases/06-local-managed-execution/06-CONTEXT.md`, `.planning/REQUIREMENTS.md`, `.planning/STATE.md`, `.planning/ROADMAP.md`, `.planning/phases/05-simulation-and-policy-gate/05-VERIFICATION.md`, `.planning/phases/05-simulation-and-policy-gate/05-05-SUMMARY.md`, `CLAUDE.md`. [VERIFIED: file reads]
- Codebase files read: `crates/executor-signer/src/lib.rs`, `crates/executor-signer/Cargo.toml`, `crates/executor-evm/src/normalize.rs`, `crates/executor-evm/src/simulate.rs`, `crates/executor-mcp/src/tools.rs`, `crates/executor-mcp/src/resources.rs`, `crates/executor-mcp/src/config.rs`, `crates/executor-mcp/src/server.rs`, `crates/executor-state/src/schema.rs`, `crates/executor-state/src/journal.rs`, `crates/executor-state/src/store.rs`, `crates/executor-core/src/schema/execution.rs`. [VERIFIED: file reads]
- Alloy local cargo registry source: `alloy-signer-local-2.0.1/src/lib.rs`, `private_key.rs`; `alloy-provider-2.0.1/src/builder.rs`, `fillers/wallet.rs`, `provider/trait.rs`, `heart.rs`. [VERIFIED: local cargo registry]
- [Alloy crate docs.rs](https://docs.rs/alloy/2.0.1) - version/docs URL from `cargo info alloy`. [CITED: docs.rs]
- [alloy-signer-local crate docs.rs](https://docs.rs/alloy-signer-local/2.0.1) - version/docs URL from `cargo info alloy-signer-local`. [CITED: docs.rs]
- [Alloy GitHub repository](https://github.com/alloy-rs/alloy) - repository URL from `cargo info`. [CITED: cargo info]

### Secondary (MEDIUM confidence)

- `cargo search alloy --limit 1` and `cargo search alloy-signer-local --limit 1` for current crates.io version visibility. [VERIFIED: crates.io search]
- GitNexus CLI availability and warning behavior. [VERIFIED: CLI probe]

### Tertiary (LOW confidence)

- Assumed internal names (`execution_actions`, `LocalSignerHandle`, exact test filters) are recommendations only. [ASSUMED]

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - Alloy versions and APIs verified from cargo info, crates.io search, existing lock/dependency tree, and local cargo registry source. [VERIFIED]
- Architecture: HIGH - Strategy/policy/simulation/state/MCP patterns verified in current code and Phase 5 verification. [VERIFIED]
- Pitfalls: MEDIUM-HIGH - Security/order pitfalls are mostly locked by CONTEXT and existing Phase 5 safety design; exact execution error taxonomy remains assumed. [VERIFIED + ASSUMED]

**Research date:** 2026-04-28  
**Valid until:** 2026-05-05 for Alloy API details; 2026-05-28 for project architecture if Phase 6 has not yet changed code.
