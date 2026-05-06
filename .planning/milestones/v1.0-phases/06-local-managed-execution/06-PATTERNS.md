# Phase 6: Local Managed Execution - Pattern Map

**Mapped:** 2026-04-28
**Files analyzed:** 17 new/modified files
**Analogs found:** 17 / 17

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `crates/executor-mcp/src/config.rs` | config | request-response | `crates/executor-mcp/src/config.rs` `[policy]` / `[evm]` sections | exact |
| `crates/executor-mcp/src/server.rs` | provider/orchestrator | request-response | `crates/executor-mcp/src/server.rs` policy fail-closed + lazy provider | exact |
| `crates/executor-mcp/src/tools.rs` | controller/orchestrator | request-response + CRUD + external I/O | `crates/executor-mcp/src/tools.rs` `strategy_run` Phase 5 gate pipeline | exact |
| `crates/executor-mcp/src/resources.rs` | route/resource | request-response | `crates/executor-mcp/src/resources.rs` `strategy://` + `journal://` readers | exact |
| `crates/executor-mcp/Cargo.toml` | config | build/dependency | `crates/executor-mcp/Cargo.toml` anvil feature / crate deps | exact |
| `crates/executor-core/src/schema/execution.rs` | model/schema | request-response | current `ExecutionGetResponse`, `RunStatus`, `StrategyRunResponse` | exact |
| `crates/executor-core/tests/schema_snapshots.rs` | test | batch/transform | schema golden tests in same file | exact |
| `crates/executor-state/src/schema.rs` | model/migration | CRUD | `journal_decisions` DDL in same file | exact |
| `crates/executor-state/src/executions.rs` | repository | CRUD | `crates/executor-state/src/journal.rs` decision repo | exact |
| `crates/executor-state/src/store.rs` | repository facade | CRUD | `StateStore` run/journal facades | exact |
| `crates/executor-state/src/runs.rs` | repository/model | CRUD | transition-guarded run lifecycle | role-match |
| `crates/executor-state/src/lib.rs` | crate boundary | transform | existing state module exports | role-match |
| `crates/executor-state/tests/executions.rs` | test | CRUD | state journal/runs round-trip tests | role-match |
| `crates/executor-signer/src/lib.rs` | service/crate boundary | external I/O | `executor-evm` provider boundary + signer scaffold | role-match |
| `crates/executor-signer/src/config.rs` | config | transform | `executor-evm/src/config.rs` typed config validation | role-match |
| `crates/executor-signer/src/error.rs` | utility/error | transform | `executor-evm/src/simulate.rs` wire-safe taxonomy discipline | role-match |
| `crates/executor-signer/src/local.rs` | service | external I/O + request-response | `executor-evm/src/provider.rs` + `simulate.rs` async Alloy adapter | role-match |
| `crates/executor-signer/Cargo.toml` | config | build/dependency | `crates/executor-evm/Cargo.toml` isolated Alloy deps | exact |
| `crates/executor-mcp/tests/stdio_handshake.rs` | integration test | request-response + external I/O | Phase 5 stdio/anvil tests in same file | exact |
| `crates/executor-evm/tests/common/anvil_fixture.rs` or MCP test helper copy | test utility | external I/O | `AnvilFixture::try_spawn` | exact |

## Pattern Assignments

### `crates/executor-mcp/src/config.rs` (config, request-response)

**Analog:** existing `[policy]` fail-closed section and `[evm]` typed builder in `crates/executor-mcp/src/config.rs`.

**Imports pattern** (lines 12-15):
```rust
use anyhow::{Context, Result};
use executor_policy::{LoadedPolicy, PolicyError};
use serde::Deserialize;
use std::path::Path;
```

**Config section pattern** (lines 17-34):
```rust
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub state: StateConfig,
    #[serde(default)]
    pub evm: EvmSection,
    #[serde(default)]
    pub policy: PolicyFileSection,
}
```

**Fail-closed optional section pattern** (lines 36-43):
```rust
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PolicyFileSection {
    /// Path to policy.toml. `None` → policy NOT loaded (D-15 fail-closed).
    pub path: Option<String>,
}
```

**Typed config builder pattern** (lines 124-134, 136-152):
```rust
impl Config {
    pub fn evm_config(&self) -> Result<executor_evm::EvmConfig, executor_evm::EvmError> {
        executor_evm::EvmConfig::from_raw(
            &self.evm.rpc_url,
            self.evm.call_timeout_ms,
            &self.evm.simulation_from,
        )
    }

    pub fn policy_config(&self) -> Result<Option<LoadedPolicy>, PolicyError> {
        let Some(path_str) = self.policy.path.as_deref() else {
            return Ok(None);
        };
        executor_policy::load_policy_from_path(Path::new(path_str)).map(Some)
    }
}
```

**Apply to Phase 6:** Add `[signer]` with `private_key_env: Option<String>` and receipt timeout field. Use `#[serde(default, deny_unknown_fields)]`. Do not add any raw private-key TOML field and do not default to an anvil key.

---

### `crates/executor-mcp/src/server.rs` (provider/orchestrator, request-response)

**Analog:** policy fail-closed boot and EVM lazy provider in `ExecutorServer`.

**Imports pattern** (lines 13-19, 31-36):
```rust
use std::sync::Arc;

use anyhow::Result;
use executor_evm::{DynProvider, EvmConfig, EvmError};
use executor_policy::LoadedPolicy;
use executor_state::StateStore;
use tokio::sync::{Mutex, RwLock};

use crate::{
    config::{Config, StateConfig},
    resources,
};
```

**State/config fields pattern** (lines 38-62):
```rust
#[derive(Clone)]
pub struct ExecutorServer {
    pub(crate) tool_router: ToolRouter<Self>,
    pub(crate) prompt_router: PromptRouter<Self>,
    pub(crate) state: Arc<Mutex<StateStore>>,
    pub(crate) evm_config: EvmConfig,
    pub(crate) evm_provider: Arc<tokio::sync::OnceCell<Arc<DynProvider>>>,
    pub(crate) chain_id_cell: Arc<tokio::sync::OnceCell<u64>>,
    pub(crate) policy: Arc<RwLock<Option<LoadedPolicy>>>,
}
```

**Fail-closed boot pattern** (lines 93-129):
```rust
pub fn new_with_full_config(
    state_cfg: &StateConfig,
    evm_config: &EvmConfig,
    full_cfg: &Config,
) -> Result<Self> {
    let mut srv = Self::new_with_config(state_cfg, evm_config)?;
    let loaded: Option<LoadedPolicy> = match full_cfg.policy_config() {
        Ok(Some(p)) => {
            tracing::info!(chains = ?p.chains_allow, raw_call_global = p.raw_call_allow_global, "policy loaded");
            Some(p)
        }
        Ok(None) => {
            tracing::warn!("[policy].path not configured — strategy_run will fail-closed with policy_not_loaded");
            None
        }
        Err(e) => {
            tracing::error!(detail = %e.detail_for_log(), kind = %e.data_kind(), "policy load failed — strategy_run will fail-closed with policy_not_loaded");
            None
        }
    };
    srv.policy = Arc::new(RwLock::new(loaded));
    Ok(srv)
}
```

**Lazy provider / cached chain id pattern** (lines 150-168):
```rust
pub async fn chain_id(&self) -> Result<u64, EvmError> {
    let cell = self.chain_id_cell.clone();
    let provider = self.evm_provider().await?;
    cell.get_or_try_init(|| async move { executor_evm::fetch_chain_id(&provider).await })
        .await
        .copied()
}

pub async fn evm_provider(&self) -> Result<Arc<DynProvider>, EvmError> {
    let cell = self.evm_provider.clone();
    let cfg = self.evm_config.clone();
    cell.get_or_try_init(|| async move { executor_evm::build_provider(&cfg) })
        .await
        .cloned()
}
```

**Apply to Phase 6:** Store signer config/handle in server without exposing private key. Prefer fail-closed runtime errors for missing/invalid signer. If signer is loaded at boot, log only env-var name and signer address, never secret value.

---

### `crates/executor-mcp/src/tools.rs` (controller/orchestrator, request-response + external I/O)

**Analog:** `strategy_run` Phase 5 pipeline in `crates/executor-mcp/src/tools.rs`.

**Imports pattern** (lines 9-33, 46-56):
```rust
use alloy_primitives::{Address, U256};
use executor_core::schema::{
    action::Action,
    execution::{
        ActionDecision, ExecutionGetResponse, ExecutionIdInput, GateVerdict, JournalActionOutcome,
        RunStatus, StrategyOutcome, StrategyRunResponse,
    },
};
use executor_evm::{
    NormalizedAction, NormalizedActionKind, SimulationFailReason, SimulationOutcome,
    normalize_action, simulate_one_latest,
};
use executor_state::{
    DecisionGate, DecisionVerdict as JournalDecisionVerdict, RegisterOutcome, StateError,
    StateStore, Strategy, StrategySummary,
};
```

**DB access pattern: never hold mutex across await** (lines 83-96):
```rust
let state = self.state.clone();
let outcome = tokio::task::spawn_blocking(move || {
    let mut store = state.blocking_lock();
    store.register_strategy(
        &input.name,
        &input.source,
        input.description.as_deref(),
        input.tags.as_deref(),
    )
})
.await
.map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
.map_err(map_state_error)?;
```

**Execution status response pattern** (lines 207-234):
```rust
async fn execution_get(
    &self,
    Parameters(input): Parameters<ExecutionIdInput>,
) -> Result<CallToolResult, McpError> {
    let run_id = input.execution_id.clone();
    let state = self.state.clone();
    let row = tokio::task::spawn_blocking(move || {
        let store = state.blocking_lock();
        store.get_run(&run_id)
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
    .map_err(map_state_error)?;

    match row {
        None => Err(map_state_error(StateError::NotFound(format!("run {}", input.execution_id)))),
        Some(r) => json_result(&ExecutionGetResponse { /* fields */ }),
    }
}
```

**`strategy_run` insertion point** (lines 361-594):
```rust
// STEP 7: Phase 5 gate pipeline (D-07): policy → simulation.
let outcome = match outcome {
    StrategyOutcome::Noop => StrategyOutcome::Noop,
    StrategyOutcome::Actions { actions, .. } => {
        let mut normalized = Vec::with_capacity(actions.len());
        for action in &actions {
            match normalize_action(action) {
                Ok(action) => normalized.push(action),
                Err(e) => { /* record_runtime_error + transition Failed */ }
            }
        }
        /* policy gate then simulation gate */
        StrategyOutcome::Actions { actions, decisions }
    }
};

record_action(&self.state, &run_id, &outcome).await?;
transition(&self.state, &run_id, RunStatus::Running, RunStatus::Succeeded).await?;
```

**Simulation gate async external I/O pattern** (lines 513-586):
```rust
for (idx, normalized_action) in normalized.iter().enumerate() {
    let Some(na) = normalized_action else { continue; };
    let sim = simulate_one_latest(
        provider.clone(),
        &self.evm_config,
        &na.tx,
        Some(self.evm_config.simulation_from),
    )
    .await;
    match sim {
        SimulationOutcome::Pass { return_bytes, gas_estimate } => { /* record pass */ }
        SimulationOutcome::Fail { reason, raw_for_log } => {
            tracing::warn!(action_index = idx, raw = %raw_for_log, "simulation gate failed");
            /* record fail + terminal SimulationDenied */
        }
    }
}
```

**Error/journal helper pattern** (lines 858-887, 889-919):
```rust
async fn record_runtime_error(
    state: &Arc<Mutex<StateStore>>,
    run_id: &str,
    detail: &str,
) -> Result<(), McpError> {
    let payload = serde_json::json!({
        "code": "strategy_runtime_error",
        "detail": detail,
    });
    record_gate_action_outcome(state, run_id, JournalActionOutcome::RuntimeError, payload).await
}

#[allow(clippy::too_many_arguments)]
async fn record_decision_row(/* ... */) -> Result<(), McpError> {
    let state = state.clone();
    let rid = run_id.to_string();
    tokio::task::spawn_blocking(move || {
        let mut store = state.blocking_lock();
        store.record_decision(&rid, action_index, gate, verdict, rule.as_deref(), detail.as_deref(), payload.as_ref())
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
    .map_err(map_state_error)?;
    Ok(())
}
```

**Apply to Phase 6:** Insert `execute_approved_actions` after all policy/simulation passes and before `record_action`/`Succeeded`. Preserve original action indices via `normalized.iter().enumerate()`. Persist tx hash immediately after broadcast and update after receipt. On execution failure/timeout, record execution row + runtime error and transition `Running -> Failed`; do not continue later actions.

---

### `crates/executor-mcp/src/resources.rs` (resource route, request-response)

**Analog:** `strategy://` and `journal://` readers in `resources.rs`.

**Imports pattern** (lines 18-33):
```rust
use std::sync::Arc;

use executor_core::schema::strategy::StrategyGetResponse;
use executor_state::{StateError, StateStore};
use rmcp::{
    ErrorData as McpError, RoleServer,
    model::{ReadResourceRequestParams, ReadResourceResult, ResourceContents},
    service::RequestContext,
};
use serde_json::json;

use crate::errors::{map_state_error, storage_error};
```

**URI dispatch pattern** (lines 90-111):
```rust
if let Some(id) = uri.strip_prefix("strategy://") {
    let id_owned = id.to_string();
    return read_strategy(uri, id_owned, state).await;
}
if let Some(rid) = uri.strip_prefix("journal://") {
    let rid_owned = rid.to_string();
    return read_journal(uri, rid_owned, state).await;
}
if uri.starts_with("execution://") {
    return Err(McpError::resource_not_found(
        format!("execution {uri} not found (Phase 6 wires receipts)"),
        Some(json!({ "uri": uri, "phase": 6 })),
    ));
}
```

**ULID boundary check and state-backed resource body pattern** (lines 166-195, 250-254):
```rust
if run_id.len() != 26 || !run_id.chars().all(|c| c.is_ascii_alphanumeric()) {
    return Err(McpError::resource_not_found(
        format!("malformed run id in uri: {uri}"),
        Some(json!({ "uri": uri, "code": "malformed_id" })),
    ));
}

let result = tokio::task::spawn_blocking(move || -> Result<_, StateError> {
    let store = state.blocking_lock();
    let exists = store.get_run(&rid_owned)?;
    if exists.is_none() {
        return Ok(None);
    }
    /* list rows */
    Ok(Some((s, a, l, d)))
})
.await
.map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
.map_err(map_state_error)?;

let body_text = serde_json::to_string(&body)
    .map_err(|e| storage_error(format!("serialize journal: {e}")))?;
Ok(ReadResourceResult::new(vec![
    ResourceContents::text(body_text, uri).with_mime_type("application/json"),
]))
```

**Apply to Phase 6:** Replace `execution://` placeholder with `read_execution`. Reuse the same report builder as `execution_get`; malformed ID remains resource_not_found `-32002` with `data.code = malformed_id`.

---

### `crates/executor-core/src/schema/execution.rs` (model/schema, request-response)

**Analog:** existing execution and strategy response schemas.

**Imports pattern** (lines 3-4):
```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
```

**Run status wire enum pattern** (lines 23-36):
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
    SimulationDenied,
    PolicyDenied,
}
```

**Tagged enum response pattern** (lines 119-129):
```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[schemars(description = "Outcome of a successful strategy run (D-08, Phase 5 D-11).")]
pub enum StrategyOutcome {
    Noop,
    Actions {
        actions: Vec<crate::schema::action::Action>,
        #[serde(default)]
        decisions: Vec<ActionDecision>,
    },
}
```

**Optional response fields pattern** (lines 148-159):
```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Response for execution_get (Phase 2 base run model).")]
pub struct ExecutionGetResponse {
    pub run_id: String,
    pub strategy_id: String,
    pub status: RunStatus,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
```

**Apply to Phase 6:** Widen `ExecutionGetResponse` with `signer_address: Option<String>` and `actions: Vec<ExecutionActionReport>` (or similar). Keep JSON-schema derivations and `serde` option defaults. Use decimal strings for gas/value-like integers.

---

### `crates/executor-state/src/schema.rs` (migration/schema, CRUD)

**Analog:** `journal_decisions` table.

**DDL pattern** (lines 79-100):
```sql
CREATE TABLE IF NOT EXISTS journal_decisions (
    id           TEXT PRIMARY KEY,
    run_id       TEXT NOT NULL REFERENCES runs(id),
    action_index INTEGER NOT NULL,
    gate         TEXT NOT NULL,
    verdict      TEXT NOT NULL,
    rule         TEXT,
    detail       TEXT,
    payload_json TEXT,
    recorded_at  TEXT NOT NULL,
    seq          INTEGER NOT NULL,
    UNIQUE (run_id, seq)
);
CREATE INDEX IF NOT EXISTS idx_journal_decisions_run_id
    ON journal_decisions(run_id);
```

**Open schema pattern** (lines 102-111):
```rust
pub(crate) fn open_conn(path: &Path) -> Result<Connection, StateError> {
    let conn = Connection::open(path)
        .map_err(|e| StateError::Storage(format!("open {}: {e}", path.display())))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;\n\
         PRAGMA synchronous = NORMAL;\n\
         PRAGMA foreign_keys = ON;",
    )?;
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(conn)
}
```

**Apply to Phase 6:** Add an idempotent `execution_actions` table to `SCHEMA_SQL`. Use `run_id` FK, `action_index`, `signer_address`, `tx_hash`, status/receipt/gas/error fields, timestamps, and `UNIQUE(run_id, action_index)`. Add an index by `run_id`.

---

### `crates/executor-state/src/executions.rs` (repository, CRUD)

**Analog:** `crates/executor-state/src/journal.rs` decision repo.

**Enum-to-wire pattern** (lines 25-57):
```rust
#[derive(Debug, Clone, Copy)]
pub enum DecisionGate {
    Policy,
    Simulation,
}

impl DecisionGate {
    fn as_str(self) -> &'static str {
        match self {
            Self::Policy => "policy",
            Self::Simulation => "simulation",
        }
    }
}
```

**Entry struct pattern** (lines 59-71):
```rust
#[derive(Debug, Clone)]
pub struct DecisionEntry {
    pub id: String,
    pub run_id: String,
    pub action_index: i64,
    pub gate: String,
    pub verdict: String,
    pub rule: Option<String>,
    pub detail: Option<String>,
    pub payload_json: Option<String>,
    pub recorded_at: String,
    pub seq: i64,
}
```

**Insert/update serialization pattern** (lines 82-121):
```rust
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_decision(
    conn: &Connection,
    run_id: &str,
    action_index: i64,
    gate: DecisionGate,
    verdict: DecisionVerdict,
    rule: Option<&str>,
    detail: Option<&str>,
    payload: Option<&serde_json::Value>,
) -> Result<String, StateError> {
    let id = ulid::Ulid::new().to_string();
    let now = super::strategies::now_rfc3339();
    let seq = next_decision_seq(conn, run_id)?;
    let payload_str = match payload {
        Some(p) => Some(serde_json::to_string(p).map_err(|e| {
            StateError::SerializationError(format!("journal_decisions.payload: {e}"))
        })?),
        None => None,
    };
    conn.execute(/* INSERT ... */, params![/* ... */])?;
    Ok(id)
}
```

**List ordering pattern** (lines 166-195):
```rust
pub(crate) fn list_decisions_for_run(
    conn: &Connection,
    run_id: &str,
) -> Result<Vec<DecisionEntry>, StateError> {
    let mut stmt = conn.prepare(
        "SELECT id, run_id, action_index, gate, verdict, rule, detail, payload_json, recorded_at, seq \
         FROM journal_decisions WHERE run_id = ?1 \
         ORDER BY recorded_at ASC, seq ASC",
    )?;
    /* query_map into typed entries */
}
```

**Apply to Phase 6:** Implement `ExecutionEntry`, `ExecutionStatus` wire conversions, `record_broadcast`, `record_receipt`, `record_execution_error`, and `list_executions_for_run`. Use `?` propagation for serialization; never silently fallback to empty/`null` rows.

---

### `crates/executor-state/src/store.rs` (repository facade, CRUD)

**Analog:** existing façade methods for runs/journals.

**StateStore doc/import pattern** (lines 1-15):
```rust
//! `StateStore` is **NOT** internally synchronised. Callers wrap this in
//! `Arc<tokio::sync::Mutex<StateStore>>` and enter a `spawn_blocking` +
//! `blocking_lock()` block before mutating calls.

use crate::{error::StateError, runs, schema, strategies};
use executor_core::schema::execution::RunStatus;
use rusqlite::Connection;
use std::path::Path;
```

**Run façade pattern** (lines 76-139):
```rust
pub fn insert_run(
    &mut self,
    strategy_id: &str,
    status: RunStatus,
) -> Result<String, StateError> {
    runs::insert_run(&self.conn, strategy_id, status)
}

pub fn get_run(&self, run_id: &str) -> Result<Option<runs::Run>, StateError> {
    runs::get_run(&self.conn, run_id)
}

pub fn update_run_status_with_transition(
    &mut self,
    run_id: &str,
    from: RunStatus,
    to: RunStatus,
) -> Result<(), StateError> {
    runs::update_run_status_with_transition(&self.conn, run_id, from, to)
}
```

**Journal façade pattern** (lines 217-271):
```rust
#[allow(clippy::too_many_arguments)]
pub fn record_decision(
    &mut self,
    run_id: &str,
    action_index: i64,
    gate: crate::journal::DecisionGate,
    verdict: crate::journal::DecisionVerdict,
    rule: Option<&str>,
    detail: Option<&str>,
    payload: Option<&serde_json::Value>,
) -> Result<String, StateError> {
    crate::journal::record_decision(
        &self.conn, run_id, action_index, gate, verdict, rule, detail, payload,
    )
}

pub fn list_decisions_for_run(
    &self,
    run_id: &str,
) -> Result<Vec<crate::journal::DecisionEntry>, StateError> {
    crate::journal::list_decisions_for_run(&self.conn, run_id)
}
```

**Apply to Phase 6:** Add an `executions` module import and thin façade methods only; keep SQL in repository module, not MCP.

---

### `crates/executor-state/src/runs.rs` (repository/model, CRUD)

**Analog:** transition-guarded lifecycle.

**Status wire conversion pattern** (lines 31-58):
```rust
fn status_to_wire(s: RunStatus) -> &'static str {
    match s {
        RunStatus::Queued => "queued",
        RunStatus::Running => "running",
        RunStatus::Succeeded => "succeeded",
        RunStatus::Failed => "failed",
        RunStatus::Canceled => "canceled",
        RunStatus::SimulationDenied => "simulation_denied",
        RunStatus::PolicyDenied => "policy_denied",
    }
}
```

**Terminal status pattern** (lines 60-68):
```rust
fn is_terminal_status(status: RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Succeeded
            | RunStatus::Failed
            | RunStatus::SimulationDenied
            | RunStatus::PolicyDenied
    )
}
```

**Transition guard pattern** (lines 139-205):
```rust
pub(crate) fn update_run_status_with_transition(
    conn: &Connection,
    run_id: &str,
    from: RunStatus,
    to: RunStatus,
) -> Result<(), StateError> {
    if is_terminal_status(from) {
        return Err(StateError::InvalidInput(format!(
            "run {run_id} is in terminal state {from:?}; transition to {to:?} is disallowed (D-12)"
        )));
    }
    let finished_at = is_terminal_status(to).then(super::strategies::now_rfc3339);
    let affected = conn.execute(
        "UPDATE runs SET status = ?1, finished_at = COALESCE(?2, finished_at) \
         WHERE id = ?3 AND status = ?4",
        params![status_to_wire(to), finished_at, run_id, status_to_wire(from)],
    )?;
    /* distinguish NotFound vs invalid transition */
    Ok(())
}
```

**Apply to Phase 6:** If `Canceled` becomes emittable or execution failure semantics change, update emittable gating deliberately. Receipt failure/timeout should transition `Running -> Failed` and populate `finished_at`.

---

### `crates/executor-signer/src/lib.rs`, `config.rs`, `error.rs`, `local.rs` (service/crate boundary, external I/O)

**Analog 1:** signer scaffold in `crates/executor-signer/src/lib.rs`.

**Current boundary** (lines 0-16):
```rust
#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
//! Signer boundary — Phase 6에서 local signer 구현.

use executor_core::schema::execution::SignedTransaction;

pub trait Signer: Send + Sync {
    // 실제 sign 메서드는 Phase 6 연구 후 확정.
}

#[doc(hidden)]
pub type _SignedTransactionAlias = SignedTransaction;
```

**Analog 2:** typed EVM config validation in `crates/executor-evm/src/config.rs`.

**Config struct + validation pattern** (lines 16-27, 46-78):
```rust
#[derive(Debug, Clone)]
pub struct EvmConfig {
    pub rpc_url: Url,
    pub call_timeout: Duration,
    pub simulation_from: Address,
}

impl EvmConfig {
    pub fn from_raw(
        rpc_url: &str,
        call_timeout_ms: u64,
        simulation_from: &str,
    ) -> Result<Self, EvmError> {
        let rpc_url: Url = rpc_url.parse().map_err(|e: url::ParseError| EvmError::Config {
            detail_for_log: format!("rpc_url parse: {e}"),
        })?;
        if !(50..=30_000).contains(&call_timeout_ms) {
            return Err(EvmError::Config {
                detail_for_log: format!("call_timeout_ms {call_timeout_ms} not in 50..=30000"),
            });
        }
        let simulation_from = parse_simulation_from(simulation_from)?;
        Ok(Self { rpc_url, call_timeout: Duration::from_millis(call_timeout_ms), simulation_from })
    }
}
```

**Analog 3:** Alloy provider boundary in `crates/executor-evm/src/provider.rs`.

**Provider builder pattern** (lines 7-20):
```rust
use std::sync::Arc;

use alloy::providers::{DynProvider, Provider, ProviderBuilder};

use crate::{EvmConfig, EvmError};

pub fn build_provider(cfg: &EvmConfig) -> Result<Arc<DynProvider>, EvmError> {
    let provider = ProviderBuilder::new()
        .connect_http(cfg.rpc_url.clone())
        .erased();
    Ok(Arc::new(provider))
}
```

**Analog 4:** async external I/O + wire-safe outcome in `crates/executor-evm/src/simulate.rs`.

**Outcome taxonomy pattern** (lines 33-69):
```rust
#[derive(Debug, Clone)]
pub enum SimulationOutcome {
    Pass { return_bytes: Bytes, gas_estimate: Option<u64> },
    Fail { reason: SimulationFailReason, raw_for_log: String },
}

#[derive(Debug, Clone)]
pub enum SimulationFailReason {
    Revert { decoded: Option<String> },
    Transport,
    Timeout,
}
```

**Timeout + raw-for-log only pattern** (lines 92-129):
```rust
let call_future = provider.call(tx_with_from).block(block);
let timeout_result = tokio::time::timeout(cfg.call_timeout, call_future).await;

match timeout_result {
    Ok(Ok(bytes)) => SimulationOutcome::Pass { return_bytes: bytes, gas_estimate: None },
    Ok(Err(e)) => {
        let raw = e.to_string();
        /* classify into stable reason, keep raw_for_log off wire */
    }
    Err(_elapsed) => SimulationOutcome::Fail {
        reason: SimulationFailReason::Timeout,
        raw_for_log: "tokio::time::timeout fired".into(),
    },
}
```

**Apply to Phase 6:** Replace scaffold with real local signer boundary. Add `LocalSignerConfig { private_key_env, receipt_timeout }`, `SignerError` with stable `data_kind()` / `detail_for_log()` style methods, and a local execution adapter using Alloy local signer + wallet-enabled provider. Return stable execution outcomes; raw provider/signer errors only go to tracing.

---

### `crates/executor-evm/src/normalize.rs` (utility/service, transform)

**Analog:** normalized action is the signer input.

**NormalizedAction shape** (lines 34-52):
```rust
#[derive(Debug, Clone)]
pub struct NormalizedAction {
    pub tx: TransactionRequest,
    pub source: NormalizedActionKind,
    pub selector: Option<[u8; 4]>,
    pub native_value: U256,
    pub erc20_amount: Option<U256>,
}
```

**Phase 6-owned fields note** (lines 36-39):
```rust
/// `Noop` is filtered out earlier — [`normalize_action`] returns
/// `Ok(None)` for it. The `tx` field has `to`, `data` and `value`
/// populated; `gas` / `nonce` / `chain_id` are intentionally NOT set
/// (Phase 6 owns signer-side completion).
```

**Apply to Phase 6:** The executor-signer loop must consume `NormalizedAction.tx` and rely on Alloy fillers for gas/nonce/chain id/from. Preserve `Option<NormalizedAction>` indices.

---

### `crates/executor-mcp/Cargo.toml`, `crates/executor-signer/Cargo.toml`, crate boundary patterns (config/build)

**Analog:** isolated dependency comments.

**executor-evm Alloy dependency pattern** (lines 9-22):
```toml
# New deps (alloy + dyn-abi family) are intentionally NOT promoted to workspace
# dependencies — only `executor-evm` consumes alloy today (Phase 4 D-01).
[dependencies]
executor-core = { path = "../executor-core" }
alloy = { version = "2.0", default-features = false, features = [
    "provider-http",
    "contract",
    "rpc-types-eth",
    "json-rpc",
    "reqwest-rustls-tls",
] }
```

**anvil-gated feature pattern** (`crates/executor-mcp/Cargo.toml` lines 13-19):
```toml
[features]
default = []
# Phase 5 Plan 05-02 / D-08 — opts integration tests in to anvil-gated paths
anvil-tests = ["alloy/node-bindings"]
```

**executor-state isolated deps pattern** (`crates/executor-state/Cargo.toml` lines 9-23):
```toml
# New deps (...) are intentionally NOT promoted to workspace dependencies — only
# `executor-state` consumes them today, mirroring the Phase 1 precedent.
[dependencies]
executor-core = { path = "../executor-core" }
rusqlite = { version = "0.39", features = ["bundled"] }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
```

**Apply to Phase 6:** Add Alloy local signer dependencies to `executor-signer`, not `executor-mcp`, unless test-only node-bindings are required. Keep `executor-core` pure-domain and free of Alloy/rmcp. `executor-state` should not depend on Alloy; persist strings.

---

### `crates/executor-mcp/tests/stdio_handshake.rs` and anvil-gated integration tests (test, request-response + external I/O)

**Analog:** Phase 5 stdio + anvil tests.

**Test imports / cfg pattern** (lines 18-27):
```rust
use anyhow::Result;
#[cfg(feature = "anvil-tests")]
use alloy::network::TransactionBuilder;
#[cfg(feature = "anvil-tests")]
use alloy::providers::Provider;
#[cfg(feature = "anvil-tests")]
use alloy::rpc::types::TransactionRequest;
#[cfg(feature = "anvil-tests")]
use alloy_primitives::Address;
use serde_json::{Value, json};
```

**Stdio tool call pattern** (lines 1191-1207):
```rust
let mut proc = spawn_server_with_state(&db_path_str).await?;
let _ = initialize(&mut proc).await?;
let r = call_tool(
    &mut proc,
    2,
    "strategy_run",
    json!({ "strategy_id": strategy_id }),
)
.await?;
let body = extract_json_result(&r);
assert_eq!(body["status"].as_str(), Some("succeeded"));
proc.child.kill().await?;
```

**Anvil-gated skip-cleanly pattern** (lines 2661-2675):
```rust
#[cfg(feature = "anvil-tests")]
#[tokio::test(flavor = "multi_thread")]
async fn strategy_run_returns_simulation_failed_when_revert() -> Result<()> {
    let Some(fixture) = alloy::node_bindings::Anvil::new()
        .chain_id(31337)
        .try_spawn()
        .ok()
    else {
        return Ok(());
    };
    let funded_accounts = fixture.addresses().to_vec();
    if funded_accounts.is_empty() {
        return Ok(());
    }
```

**Policy + RPC server config helper pattern** (lines 1161-1182):
```rust
async fn spawn_server_with_policy_and_rpc(
    db_path: &std::path::Path,
    policy_path: &std::path::Path,
    rpc_url: &str,
) -> Result<common::ServerProc> {
    spawn_server_with_config_text(&format!(
        r#"[state]
path = "{}"

[policy]
path = "{}"

[evm]
rpc_url = "{}"
call_timeout_ms = 1000
"#,
        db_path.display(),
        policy_path.display(),
        rpc_url,
    ))
    .await
}
```

**Journal resource assertion pattern** (lines 2717-2755):
```rust
let body = extract_json_result(&r);
let run_id = body["run_id"].as_str().expect("run_id");
let journal = read_journal_resource(&mut proc, 3, run_id).await?;
assert_decision_row(&journal, 0, "policy", "pass", None);
assert_decision_row(&journal, 0, "simulation", "pass", None);
```

**Apply to Phase 6:** Add tests for missing signer fail-closed, valid env key signing, broadcast tx hash persistence, receipt status/gas persistence, and `execution_get`/`execution://{run_id}` parity. For anvil tests, use fixture private keys only in env variables and config only references the env var name.

---

### `crates/executor-evm/tests/common/anvil_fixture.rs` (test utility, external I/O)

**Analog:** reusable anvil fixture.

**Skip-cleanly contract** (lines 1-8):
```rust
//! Skip-cleanly contract:
//! - If `ANVIL_RPC_URL` is set, use that URL (no spawn). Tests that depend
//!   on anvil-pre-funded accounts skip in this mode (`funded_accounts` empty).
//! - Otherwise call `Anvil::new().chain_id(31337).try_spawn()`. On failure
//!   (binary missing), `eprintln!` a skip message and return `None`.
```

**Fixture implementation** (lines 21-51):
```rust
impl AnvilFixture {
    pub fn try_spawn() -> Option<Self> {
        if let Ok(url) = std::env::var("ANVIL_RPC_URL") {
            let rpc_url: Url = url.parse().ok()?;
            return Some(Self {
                instance: None,
                rpc_url,
                funded_accounts: vec![],
            });
        }
        match Anvil::new().chain_id(31337).try_spawn() {
            Ok(instance) => {
                let rpc_url = instance.endpoint_url();
                let funded = instance.addresses().to_vec();
                Some(Self { instance: Some(instance), rpc_url, funded_accounts: funded })
            }
            Err(_e) => {
                eprintln!("[skip] anvil binary not on PATH; install foundry to run anvil-tests");
                None
            }
        }
    }
}
```

**Apply to Phase 6:** Either copy this helper into MCP tests or expose it via a test-fixture feature. Do not panic when anvil is missing. If using `ANVIL_RPC_URL`, skip tests requiring known fixture private keys unless a separate private-key env var is supplied.

## Shared Patterns

### Fail-closed security config

**Source:** `crates/executor-mcp/src/server.rs` lines 93-129 and `crates/executor-mcp/src/config.rs` lines 36-43.

**Apply to:** `[signer]` config, server construction, `strategy_run` execution boundary.

Pattern: optional security-critical config may let the server boot, but `strategy_run` must fail closed with a stable `-32017` runtime error before signing/broadcasting. Log only non-secret config metadata and stable error kinds.

### Async DB access

**Source:** `crates/executor-mcp/src/tools.rs` lines 83-96 and `crates/executor-state/src/store.rs` lines 1-5.

**Apply to:** all execution row writes/reads from MCP async code.

Pattern: clone `Arc<Mutex<StateStore>>`, enter `tokio::task::spawn_blocking`, use `state.blocking_lock()`, and drop the guard before any network `.await`.

### MCP response shaping

**Source:** `crates/executor-mcp/src/tools.rs` lines 669-672 and `resources.rs` lines 250-254.

```rust
fn json_result<T: serde::Serialize>(value: &T) -> Result<CallToolResult, McpError> {
    let body = serde_json::to_string(value)
        .map_err(|e| storage_error(format!("serialize response: {e}")))?;
    Ok(CallToolResult::success(vec![Content::text(body)]))
}
```

Apply the same typed-struct serialization to `execution_get`; resource readers should use `ResourceContents::text(body_text, uri).with_mime_type("application/json")`.

### Wire-safe error taxonomy

**Source:** `crates/executor-evm/src/simulate.rs` lines 47-58 and 101-124.

Pattern: raw Alloy/provider errors go to `raw_for_log`/tracing only. MCP responses and persisted execution error fields use stable strings such as `signer_not_configured`, `invalid_private_key`, `broadcast_failed`, `receipt_timeout`, `receipt_failed`, `receipt_missing`.

### Sequential action index preservation

**Source:** `crates/executor-mcp/src/tools.rs` lines 383-405 and 437-440.

```rust
let mut normalized = Vec::with_capacity(actions.len());
for action in &actions {
    match normalize_action(action) {
        Ok(action) => normalized.push(action),
        Err(e) => { /* fail */ }
    }
}

for (idx, normalized_action) in normalized.iter().enumerate() {
    let Some(na) = normalized_action else {
        continue;
    };
    /* idx is original action index */
}
```

Apply this exact pattern to signing/broadcasting; never compact away noop positions before persisting `action_index`.

### Crate boundaries

**Source:** `crates/executor-evm/Cargo.toml` lines 9-22, `crates/executor-state/Cargo.toml` lines 9-23, `crates/executor-signer/src/lib.rs` lines 0-16.

- `executor-core`: schemas only; no rmcp, SQLite, Alloy provider, or private-key dependencies.
- `executor-state`: SQLite and string persistence only; no Alloy types in row structs.
- `executor-signer`: local signer/provider execution boundary; owns Alloy local signer dependencies.
- `executor-mcp`: orchestration and MCP wire mapping; no raw private key storage or exposure to JS.

## No Analog Found

None. Every planned Phase 6 surface has a close in-code analog. The only new semantic area is local private-key signing, but the existing `executor-signer` scaffold plus `executor-evm` Alloy provider/simulation adapter provide the crate-boundary and async-I/O patterns.

## Metadata

**Analog search scope:** `crates/executor-mcp`, `crates/executor-core`, `crates/executor-state`, `crates/executor-signer`, `crates/executor-evm`, anvil-gated integration tests.
**Files scanned:** 20 source/test/config files plus Phase 6 context/research/state/requirements.
**Pattern extraction date:** 2026-04-28
