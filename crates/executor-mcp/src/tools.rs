//! `#[tool_router]` impl block — 8 MCP tools.
//!
//! Phase transition map (Phase 2 delivers the first two groups):
//!   - Real storage-backed: strategy_register, strategy_list, strategy_get,
//!     strategy_delete, execution_get (returns not_found until runs are
//!     inserted — Plan 02-03 wires `strategy_run_once` to start emitting runs).
//!   - Still placeholders: strategy_run_once (Phase 6), policy_get (Phase 5,
//!     returns empty shape), policy_update (Phase 5, unimplemented_err).

use alloy_primitives::{Address, U256};
use executor_core::schema::{
    action::Action,
    execution::{
        ActionDecision, ExecutionGetResponse, ExecutionIdInput, GateVerdict, JournalActionOutcome,
        RunStatus, StrategyOutcome, StrategyRunResponse,
    },
    policy::PolicyUpdateInput,
    strategy::{
        StrategyDeleteResponse, StrategyGetInput, StrategyGetResponse, StrategyIdInput,
        StrategyListItem, StrategyListResponse, StrategyRegisterInput, StrategyRegisterResponse,
        StrategyRunInput,
    },
};
use executor_evm::{
    NormalizedAction, NormalizedActionKind, SimulationFailReason, SimulationOutcome,
    normalize_action, simulate_one_latest,
};
use executor_policy::{
    Decision, DecisionVerdict, LoadedPolicy, NormalizedActionKindCopy, evaluate,
};
use executor_state::{
    DecisionGate, DecisionVerdict as JournalDecisionVerdict, RegisterOutcome, StateError,
    StateStore, Strategy, StrategySummary,
};
use rmcp::{
    ErrorData as McpError,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content},
    tool, tool_router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use strategy_js::{RuntimeContext, RuntimeError, Sandbox};
use tokio::sync::Mutex;

use crate::{
    errors::{
        invalid_params, map_evm_error, map_policy_error, map_runtime_error, map_simulation_error,
        map_state_error, policy_not_loaded, storage_error, strategy_deleted,
        strategy_invalid_output, unimplemented_err,
    },
    server::ExecutorServer,
    validation::{
        validate_action_kind_allowlisted, validate_register, validate_strategy_id_format,
    },
};

/// `strategy_list` input — single optional boolean. Declared inline because
/// it is not shared with `executor-core` (no schema golden needed — the
/// empty-args shape is invariant enough).
#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StrategyListInput {
    #[serde(default)]
    pub include_deleted: Option<bool>,
}

#[tool_router(vis = "pub(crate)")]
impl ExecutorServer {
    // ─────────── STRATEGY TOOLS (Phase 2 — storage-backed) ───────────

    #[tool(
        name = "strategy_register",
        description = "Register a JavaScript strategy (content-addressed; idempotent on same source; returns `already_exists` + `existing_*` fields when the source was previously registered)."
    )]
    async fn strategy_register(
        &self,
        Parameters(input): Parameters<StrategyRegisterInput>,
    ) -> Result<CallToolResult, McpError> {
        // 1. D-09 handler-side re-check (schema maxLength is advisory).
        validate_register(&input).map_err(invalid_params)?;

        // 2. Hand the blocking DB call to the blocking pool (Pattern 2).
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

        // 3. Shape the response (D-01b / D-07).
        let resp = match outcome {
            RegisterOutcome::Created(s) => StrategyRegisterResponse {
                strategy_id: s.id,
                name: s.name,
                created_at: s.created_at,
                already_exists: false,
                existing_name: None,
                existing_description: None,
                existing_tags: None,
                deleted_at: s.deleted_at,
            },
            RegisterOutcome::AlreadyExists(s) => StrategyRegisterResponse {
                strategy_id: s.id.clone(),
                name: s.name.clone(),
                created_at: s.created_at.clone(),
                already_exists: true,
                existing_name: Some(s.name),
                existing_description: s.description,
                existing_tags: s.tags,
                deleted_at: s.deleted_at,
            },
        };
        json_result(&resp)
    }

    #[tool(
        name = "strategy_list",
        description = "List registered strategies. Excludes the `source` field per-item to keep responses small. Pass `include_deleted: true` to also return soft-deleted rows."
    )]
    async fn strategy_list(
        &self,
        Parameters(input): Parameters<StrategyListInput>,
    ) -> Result<CallToolResult, McpError> {
        let include_deleted = input.include_deleted.unwrap_or(false);
        let state = self.state.clone();
        let summaries = tokio::task::spawn_blocking(move || {
            let store = state.blocking_lock();
            store.list_strategies(include_deleted)
        })
        .await
        .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
        .map_err(map_state_error)?;

        let resp = StrategyListResponse {
            strategies: summaries.into_iter().map(summary_to_item).collect(),
        };
        json_result(&resp)
    }

    #[tool(
        name = "strategy_get",
        description = "Get a strategy by id or by name. Pass exactly one of `strategy_id` or `name`. Id lookup includes soft-deleted rows (deleted_at populated); name lookup returns active rows only."
    )]
    async fn strategy_get(
        &self,
        Parameters(input): Parameters<StrategyGetInput>,
    ) -> Result<CallToolResult, McpError> {
        let state = self.state.clone();
        let row = tokio::task::spawn_blocking(move || {
            let store = state.blocking_lock();
            match input {
                StrategyGetInput::ById { strategy_id } => store.get_strategy_by_id(&strategy_id),
                StrategyGetInput::ByName { name } => store.get_strategy_by_name(&name),
            }
        })
        .await
        .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
        .map_err(map_state_error)?;

        match row {
            None => Err(map_state_error(StateError::NotFound("strategy".into()))),
            Some(s) => json_result(&strategy_to_get_response(s)),
        }
    }

    #[tool(
        name = "strategy_delete",
        description = "Soft-delete a registered strategy. Idempotent: repeat calls return the same deleted_at."
    )]
    async fn strategy_delete(
        &self,
        Parameters(input): Parameters<StrategyIdInput>,
    ) -> Result<CallToolResult, McpError> {
        // D-09a: reject malformed ids at the boundary.
        validate_strategy_id_format(&input.strategy_id).map_err(invalid_params)?;

        let id = input.strategy_id.clone();
        let state = self.state.clone();
        let deleted_at = tokio::task::spawn_blocking(move || {
            let mut store = state.blocking_lock();
            store.soft_delete_strategy(&id)
        })
        .await
        .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
        .map_err(map_state_error)?;

        json_result(&StrategyDeleteResponse {
            strategy_id: input.strategy_id,
            deleted_at,
        })
    }

    // ─────────── EXECUTION TOOL (Phase 2 partial — real DB lookup, not_found until Plan 02-03 run insert) ───────────

    #[tool(
        name = "execution_get",
        description = "Get a run by id. Returns not_found until a `strategy_run` call has inserted runs."
    )]
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
            None => Err(map_state_error(StateError::NotFound(format!(
                "run {}",
                input.execution_id
            )))),
            Some(r) => json_result(&ExecutionGetResponse {
                run_id: r.id,
                strategy_id: r.strategy_id,
                status: r.status,
                started_at: r.started_at,
                finished_at: r.finished_at,
                error: r.error,
            }),
        }
    }

    // ─────────── STILL-PLACEHOLDER TOOLS (Phase 5 / 6) ───────────

    #[tool(
        name = "strategy_run",
        description = "Execute a registered JavaScript strategy once in a sandbox. \
                       Returns the validated `Action[]` or `noop`. Runtime / validation \
                       errors become structured MCP errors with a `run_id` reference \
                       for journal lookup via `execution_get` and `journal://{run_id}`."
    )]
    async fn strategy_run(
        &self,
        Parameters(input): Parameters<StrategyRunInput>,
    ) -> Result<CallToolResult, McpError> {
        // STEP 1: validate input format (D-09).
        validate_strategy_id_format(&input.strategy_id).map_err(invalid_params)?;

        // STEP B (early — D-15 fail-closed): snapshot the policy. Cloning here
        // keeps the RwLock guard out of the spawn_blocking / .await boundary
        // (D-15d mutex hygiene). Defer the `None → policy_not_loaded` decision
        // until after STEP 3 so the error envelope can carry `run_id`.
        let policy_snapshot: Option<LoadedPolicy> = self.policy.read().await.clone();

        // STEP 2: load strategy + check soft-delete.
        let state = self.state.clone();
        let sid_for_load = input.strategy_id.clone();
        let strategy: Strategy = tokio::task::spawn_blocking(move || {
            let store = state.blocking_lock();
            store.get_strategy_by_id(&sid_for_load)
        })
        .await
        .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
        .map_err(map_state_error)?
        .ok_or_else(|| {
            map_state_error(StateError::NotFound(format!(
                "strategy {}",
                input.strategy_id
            )))
        })?;
        if strategy.deleted_at.is_some() {
            return Err(strategy_deleted(&strategy.id));
        }

        // STEP 3: insert run (Queued).
        let state = self.state.clone();
        let sid_for_run = strategy.id.clone();
        let run_id: String = tokio::task::spawn_blocking(move || {
            let mut store = state.blocking_lock();
            store.insert_run(&sid_for_run, RunStatus::Queued)
        })
        .await
        .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
        .map_err(map_state_error)?;

        // STEP 4: Queued → Running (D-12).
        transition(&self.state, &run_id, RunStatus::Queued, RunStatus::Running).await?;

        // STEP 5: spawn_blocking { Sandbox::execute + RuntimeContext::flush }
        // Phase 4 D-04: lazy-init the alloy provider BEFORE spawn_blocking so
        // any config error surfaces as a typed Mcp error (not a cryptic
        // exception thrown from inside the JS sandbox). Server boot is still
        // independent of devnet liveness — this is the FIRST opportunity the
        // provider would be touched on this run, and a bad URL here is a
        // legitimate runtime config error to surface.
        //
        // WR-06: URL/timeout validation already ran at server boot
        // (`from_config → evm_config()? → EvmConfig::from_raw`). The only way
        // `evm_provider()` can fail here is a near-impossible reqwest
        // connection-builder failure on an already-parsed URL. If that ever
        // happens, surface it as -32017 evm_rpc_error rather than swallowing
        // with `.ok()` and producing a confusing "no provider configured"
        // message later from the host binding.
        let evm_provider = match self.evm_provider().await {
            Ok(p) => Some(p),
            Err(e) => return Err(map_evm_error(e, &run_id)),
        };
        let evm_config = self.evm_config.clone();
        let state_for_run = self.state.clone();
        let source = strategy.source.clone();
        let sid_for_ctx = strategy.id.clone();
        let sname_for_ctx = strategy.name.clone();
        let rid_for_ctx = run_id.clone();
        let exec_result: Result<serde_json::Value, RuntimeError> =
            tokio::task::spawn_blocking(move || -> Result<serde_json::Value, RuntimeError> {
                let mut runtime_ctx = RuntimeContext::new(
                    state_for_run,
                    sid_for_ctx,
                    sname_for_ctx,
                    rid_for_ctx,
                    RuntimeContext::default_clock(),
                )
                .with_evm(evm_provider, evm_config);
                let r = Sandbox::execute(&source, &mut runtime_ctx);
                if let Err(flush_err) = runtime_ctx.flush() {
                    tracing::warn!(?flush_err, "RuntimeContext::flush failed after execute");
                }
                r
            })
            .await
            .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?;

        // STEP 6: validate output OR map runtime error.
        let outcome = match exec_result {
            Ok(json) => match validate_strategy_output(&json) {
                Ok(out) => out,
                Err(detail) => {
                    record_validation_error(&self.state, &run_id, &detail, &json).await?;
                    transition(&self.state, &run_id, RunStatus::Running, RunStatus::Failed).await?;
                    return Err(strategy_invalid_output(detail, &run_id));
                }
            },
            Err(RuntimeError::InvalidOutput { detail }) => {
                record_validation_error(&self.state, &run_id, &detail, &serde_json::Value::Null)
                    .await?;
                transition(&self.state, &run_id, RunStatus::Running, RunStatus::Failed).await?;
                return Err(strategy_invalid_output(detail, &run_id));
            }
            Err(other) => {
                let detail = other.to_string();
                record_runtime_error(&self.state, &run_id, &detail).await?;
                transition(&self.state, &run_id, RunStatus::Running, RunStatus::Failed).await?;
                return Err(map_runtime_error(other, &run_id));
            }
        };

        // STEP 7: Phase 5 gate pipeline (D-07): policy → simulation.
        let outcome = match outcome {
            StrategyOutcome::Noop => StrategyOutcome::Noop,
            StrategyOutcome::Actions { actions, .. } => {
                let all_noop = actions.iter().all(|action| matches!(action, Action::Noop));
                if policy_snapshot.is_none() && !all_noop {
                    record_runtime_error(
                        &self.state,
                        &run_id,
                        "policy violation: policy file not loaded — set [policy].path in config",
                    )
                    .await?;
                    transition(
                        &self.state,
                        &run_id,
                        RunStatus::Running,
                        RunStatus::PolicyDenied,
                    )
                    .await?;
                    return Err(policy_not_loaded(&run_id));
                }

                let mut normalized = Vec::with_capacity(actions.len());
                for action in &actions {
                    match normalize_action(action) {
                        Ok(action) => normalized.push(action),
                        Err(e) => {
                            let detail = e.to_string();
                            record_runtime_error(&self.state, &run_id, &detail).await?;
                            transition(&self.state, &run_id, RunStatus::Running, RunStatus::Failed)
                                .await?;
                            return Err(map_evm_error(e, &run_id));
                        }
                    }
                }

                // Noop actions normalize to None and do not enter the gate pipeline
                // (Phase 5 research Q-1). Only all-Noop arrays may skip policy and
                // simulation when no policy is configured.
                if normalized.iter().all(Option::is_none) {
                    StrategyOutcome::Actions {
                        actions,
                        decisions: Vec::new(),
                    }
                } else {
                    let policy = match policy_snapshot {
                        Some(p) => p,
                        None => {
                            record_runtime_error(
                                &self.state,
                                &run_id,
                                "policy violation: policy file not loaded — set [policy].path in config",
                            )
                            .await?;
                            transition(
                                &self.state,
                                &run_id,
                                RunStatus::Running,
                                RunStatus::PolicyDenied,
                            )
                            .await?;
                            return Err(policy_not_loaded(&run_id));
                        }
                    };

                    let provider = self
                        .evm_provider()
                        .await
                        .map_err(|e| map_evm_error(e, &run_id))?;
                    let chain_id = self
                        .chain_id()
                        .await
                        .map_err(|e| map_evm_error(e, &run_id))?;

                    let mut erc20_tally: HashMap<(u64, Address), U256> = HashMap::new();
                    let mut decisions = Vec::new();
                    for (idx, normalized_action) in normalized.iter().enumerate() {
                        let Some(na) = normalized_action else {
                            continue;
                        };
                        let decision =
                            decision_from_normalized(chain_id, idx as u32, na).map_err(|e| {
                                storage_error(format!("normalized action missing tx.to: {e}"))
                            })?;
                        let verdict = evaluate(&policy, &decision, &mut erc20_tally);
                        match &verdict {
                            DecisionVerdict::Allow => {
                                record_decision_row(
                                    &self.state,
                                    &run_id,
                                    idx as i64,
                                    DecisionGate::Policy,
                                    JournalDecisionVerdict::Pass,
                                    None,
                                    None,
                                    Some(policy_payload(&decision)),
                                )
                                .await?;
                                decisions.push(ActionDecision {
                                    action_index: idx as u32,
                                    policy: GateVerdict::Pass,
                                    simulation: GateVerdict::Skipped,
                                });
                            }
                            DecisionVerdict::Deny { rule, detail } => {
                                record_decision_row(
                                    &self.state,
                                    &run_id,
                                    idx as i64,
                                    DecisionGate::Policy,
                                    JournalDecisionVerdict::Fail,
                                    Some(rule.as_ref()),
                                    Some(detail.as_str()),
                                    Some(policy_payload(&decision)),
                                )
                                .await?;
                                record_decision_row(
                                    &self.state,
                                    &run_id,
                                    idx as i64,
                                    DecisionGate::Simulation,
                                    JournalDecisionVerdict::Skipped,
                                    None,
                                    Some("simulation skipped: policy denied action"),
                                    Some(simulation_skipped_payload(&decision, rule.as_ref())),
                                )
                                .await?;
                                record_gate_action_outcome(
                                    &self.state,
                                    &run_id,
                                    JournalActionOutcome::PolicyDenied,
                                    serde_json::json!({
                                        "code": "strategy_runtime_error",
                                        "kind": "policy_violation",
                                        "action_index": idx,
                                        "rule": rule.as_ref(),
                                        "detail": detail,
                                    }),
                                )
                                .await?;
                                transition(
                                    &self.state,
                                    &run_id,
                                    RunStatus::Running,
                                    RunStatus::PolicyDenied,
                                )
                                .await?;
                                return Err(map_policy_error(&verdict, idx as u32, &run_id));
                            }
                        }
                    }

                    let mut decision_pos = 0usize;
                    for (idx, normalized_action) in normalized.iter().enumerate() {
                        let Some(na) = normalized_action else {
                            continue;
                        };
                        let sim = simulate_one_latest(
                            provider.clone(),
                            &self.evm_config,
                            &na.tx,
                            Some(self.evm_config.simulation_from),
                        )
                        .await;
                        match sim {
                            SimulationOutcome::Pass {
                                return_bytes,
                                gas_estimate,
                            } => {
                                record_decision_row(
                                    &self.state,
                                    &run_id,
                                    idx as i64,
                                    DecisionGate::Simulation,
                                    JournalDecisionVerdict::Pass,
                                    None,
                                    None,
                                    Some(simulation_pass_payload(&return_bytes, gas_estimate)),
                                )
                                .await?;
                                if let Some(d) = decisions.get_mut(decision_pos) {
                                    d.simulation = GateVerdict::Pass;
                                }
                            }
                            SimulationOutcome::Fail {
                                reason,
                                raw_for_log,
                            } => {
                                tracing::warn!(action_index = idx, raw = %raw_for_log, "simulation gate failed");
                                let rule = simulation_rule(&reason);
                                let detail = simulation_detail(&reason);
                                record_decision_row(
                                    &self.state,
                                    &run_id,
                                    idx as i64,
                                    DecisionGate::Simulation,
                                    JournalDecisionVerdict::Fail,
                                    Some(rule),
                                    Some(detail.as_str()),
                                    Some(simulation_fail_payload(&reason)),
                                )
                                .await?;
                                record_gate_action_outcome(
                                    &self.state,
                                    &run_id,
                                    JournalActionOutcome::SimulationFailure,
                                    serde_json::json!({
                                        "code": "strategy_runtime_error",
                                        "kind": "simulation_failure",
                                        "action_index": idx,
                                        "rule": rule,
                                        "detail": detail,
                                    }),
                                )
                                .await?;
                                transition(
                                    &self.state,
                                    &run_id,
                                    RunStatus::Running,
                                    RunStatus::SimulationDenied,
                                )
                                .await?;
                                return Err(map_simulation_error(&reason, idx as u32, &run_id));
                            }
                        }
                        decision_pos += 1;
                    }

                    StrategyOutcome::Actions { actions, decisions }
                }
            }
        };

        record_action(&self.state, &run_id, &outcome).await?;

        // STEP 8: Running → Succeeded.
        transition(
            &self.state,
            &run_id,
            RunStatus::Running,
            RunStatus::Succeeded,
        )
        .await?;

        // STEP 9: re-read run row, build response.
        let state = self.state.clone();
        let rid_for_get = run_id.clone();
        let run = tokio::task::spawn_blocking(move || {
            let store = state.blocking_lock();
            store.get_run(&rid_for_get)
        })
        .await
        .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
        .map_err(map_state_error)?
        .ok_or_else(|| storage_error("run row vanished between insert and get"))?;

        json_result(&StrategyRunResponse {
            run_id: run.id,
            strategy_id: run.strategy_id,
            status: run.status,
            started_at: run.started_at,
            finished_at: run.finished_at.unwrap_or_default(),
            outcome,
        })
    }

    #[tool(
        name = "policy_update",
        description = "Replace the current policy. NOT YET IMPLEMENTED — lands in Phase 5."
    )]
    async fn policy_update(
        &self,
        Parameters(_input): Parameters<PolicyUpdateInput>,
    ) -> Result<CallToolResult, McpError> {
        Err(unimplemented_err("policy_update", 5))
    }

    #[tool(
        name = "policy_get",
        description = "Get the current loaded policy. Returns `{loaded: false, reason: ...}` when no policy is configured (D-15 fail-closed)."
    )]
    async fn policy_get(&self) -> Result<CallToolResult, McpError> {
        let guard = self.policy.read().await;
        let body = match &*guard {
            Some(loaded) => {
                let policy_json = serde_json::to_value(loaded).map_err(|e| {
                    // MR-03: never silently fall back to {} on serde failure.
                    tracing::warn!(detail = %e, "policy_get: failed to serialize LoadedPolicy");
                    storage_error(format!("policy_get serialize: {e}"))
                })?;
                serde_json::json!({
                    "loaded": true,
                    "policy": policy_json,
                })
            }
            None => serde_json::json!({
                "loaded": false,
                "reason": "policy not loaded (set [policy].path in config or fix policy.toml; see tracing logs)",
            }),
        };
        let body_str = serde_json::to_string(&body)
            .map_err(|e| storage_error(format!("policy_get encode: {e}")))?;
        Ok(CallToolResult::success(vec![Content::text(body_str)]))
    }
}

// ─────────── helpers ───────────

fn json_result<T: serde::Serialize>(value: &T) -> Result<CallToolResult, McpError> {
    let body = serde_json::to_string(value)
        .map_err(|e| storage_error(format!("serialize response: {e}")))?;
    Ok(CallToolResult::success(vec![Content::text(body)]))
}

fn summary_to_item(s: StrategySummary) -> StrategyListItem {
    StrategyListItem {
        strategy_id: s.id,
        name: s.name,
        description: s.description,
        tags: s.tags,
        created_at: s.created_at,
        deleted_at: s.deleted_at,
    }
}

fn strategy_to_get_response(s: Strategy) -> StrategyGetResponse {
    StrategyGetResponse {
        strategy_id: s.id,
        name: s.name,
        source: s.source,
        description: s.description,
        tags: s.tags,
        created_at: s.created_at,
        deleted_at: s.deleted_at,
    }
}

// ─────────── strategy_run helpers ───────────

/// Validate the strategy's return JSON against the Phase-3 contract:
/// `"noop"` (string) | `Action[]` (deserializable). Returns `Err(detail)`
/// if the shape is unsupported; the detail is agent-facing.
fn validate_strategy_output(v: &serde_json::Value) -> Result<StrategyOutcome, String> {
    match v {
        serde_json::Value::String(s) if s == "noop" => Ok(StrategyOutcome::Noop),
        serde_json::Value::Array(items) => {
            // Phase 5 D-12 / D-18 / BR-02 carry-forward: cap Action[] length
            // at the JSON-output gate (NOT only at the strategy-js builder).
            // A strategy returning more than MAX_ACTIONS_PER_RUN actions
            // surfaces as -32018 STRATEGY_INVALID_OUTPUT with stable detail.
            if items.len() > crate::validation::MAX_ACTIONS_PER_RUN {
                return Err(format!(
                    "actions length {} exceeds MAX_ACTIONS_PER_RUN {}",
                    items.len(),
                    crate::validation::MAX_ACTIONS_PER_RUN
                ));
            }
            // Phase-4 D-09 pre-pass: walk each element, extract `kind`, and
            // reject non-allowlisted kinds with a CLEAR error before serde
            // gets a chance to emit a less-specific "unknown variant"
            // message. Serde still enforces deny_unknown_fields per
            // variant struct (defense in depth).
            for (i, item) in items.iter().enumerate() {
                let kind = item
                    .as_object()
                    .and_then(|o| o.get("kind"))
                    .and_then(|k| k.as_str())
                    .ok_or_else(|| format!("action at index {i} missing required `kind` field"))?;
                validate_action_kind_allowlisted(kind)
                    .map_err(|e| format!("action at index {i}: {e}"))?;
            }
            // MR-03 carry-forward: ?-propagate serde failures (no silent
            // fallback to empty Vec).
            let actions: Vec<Action> = items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    serde_json::from_value::<Action>(item.clone())
                        .map_err(|e| format!("invalid action at index {i}: {e}"))
                })
                .collect::<Result<_, _>>()?;
            // BR-02: D-08 mandates the 64 KiB ABI cap is enforced at BOTH
            // builder time AND serde-deserialization (validate-strategy-output)
            // time. The builder path runs `dry_run_abi_encode` inside
            // `ctx.actions.contractCall`, but a strategy that hand-builds
            // `{kind:"contract_call", abi:"...1 MiB..."}` bypasses the
            // builder. Re-run `dry_run_abi_encode` here so the cap is
            // enforced regardless of the construction path. Per MR-01
            // carry-forward: the wire detail is the stable EvmError Display
            // (e.g. `"evm encode error: abi_oversize"`) — never raw alloy /
            // serde text.
            for (i, action) in actions.iter().enumerate() {
                if let Action::ContractCall(cc) = action
                    && let Err(e) =
                        executor_evm::action::dry_run_abi_encode(&cc.abi, &cc.function, &cc.args)
                {
                    return Err(format!("action[{i}] (contract_call): {e}"));
                }
            }
            Ok(StrategyOutcome::Actions {
                actions,
                decisions: Vec::new(),
            })
        }
        other => Err(format!(
            "expected `\"noop\"` or `Action[]`, got {}",
            json_type_name(other)
        )),
    }
}

fn json_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

async fn transition(
    state: &Arc<Mutex<StateStore>>,
    run_id: &str,
    from: RunStatus,
    to: RunStatus,
) -> Result<(), McpError> {
    let state = state.clone();
    let rid = run_id.to_string();
    tokio::task::spawn_blocking(move || {
        let mut store = state.blocking_lock();
        store.update_run_status_with_transition(&rid, from, to)
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
    .map_err(map_state_error)
}

async fn record_action(
    state: &Arc<Mutex<StateStore>>,
    run_id: &str,
    outcome: &StrategyOutcome,
) -> Result<(), McpError> {
    let (journal_outcome, payload_json): (JournalActionOutcome, String) = match outcome {
        StrategyOutcome::Noop => (JournalActionOutcome::Noop, "\"noop\"".to_string()),
        StrategyOutcome::Actions { actions, .. } => {
            // MR-03: never silently fall back to "[]" on serde failure — the
            // journal is the audit trail ("모든 실행은 기록으로 남는다") and a
            // legitimate empty-array success run is indistinguishable from a
            // swallowed error. Propagate as StateError::SerializationError so
            // the wire emits storage_error and operator forensics get the raw
            // serde detail via tracing.
            let payload = serde_json::to_string(actions).map_err(|e| {
                map_state_error(StateError::SerializationError(format!(
                    "journal_actions.payload (Vec<Action>): {e}"
                )))
            })?;
            (JournalActionOutcome::Actions, payload)
        }
    };
    let state = state.clone();
    let rid = run_id.to_string();
    tokio::task::spawn_blocking(move || {
        let mut store = state.blocking_lock();
        store.record_action_outcome(&rid, journal_outcome, &payload_json)
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
    .map_err(map_state_error)?;
    Ok(())
}

async fn record_validation_error(
    state: &Arc<Mutex<StateStore>>,
    run_id: &str,
    detail: &str,
    raw_json: &serde_json::Value,
) -> Result<(), McpError> {
    let payload = serde_json::json!({
        "code": "strategy_invalid_output",
        "detail": detail,
        "raw": raw_json,
    });
    let payload_json = payload.to_string();
    let state = state.clone();
    let rid = run_id.to_string();
    tokio::task::spawn_blocking(move || {
        let mut store = state.blocking_lock();
        store.record_action_outcome(&rid, JournalActionOutcome::ValidationError, &payload_json)
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
    .map_err(map_state_error)?;
    Ok(())
}

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

async fn record_gate_action_outcome(
    state: &Arc<Mutex<StateStore>>,
    run_id: &str,
    outcome: JournalActionOutcome,
    payload: serde_json::Value,
) -> Result<(), McpError> {
    let payload_json = payload.to_string();
    let state = state.clone();
    let rid = run_id.to_string();
    tokio::task::spawn_blocking(move || {
        let mut store = state.blocking_lock();
        store.record_action_outcome(&rid, outcome, &payload_json)
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
    .map_err(map_state_error)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn record_decision_row(
    state: &Arc<Mutex<StateStore>>,
    run_id: &str,
    action_index: i64,
    gate: DecisionGate,
    verdict: JournalDecisionVerdict,
    rule: Option<&str>,
    detail: Option<&str>,
    payload: Option<serde_json::Value>,
) -> Result<(), McpError> {
    let state = state.clone();
    let rid = run_id.to_string();
    let rule = rule.map(str::to_string);
    let detail = detail.map(str::to_string);
    tokio::task::spawn_blocking(move || {
        let mut store = state.blocking_lock();
        store.record_decision(
            &rid,
            action_index,
            gate,
            verdict,
            rule.as_deref(),
            detail.as_deref(),
            payload.as_ref(),
        )
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
    .map_err(map_state_error)?;
    Ok(())
}

fn decision_from_normalized(
    chain_id: u64,
    action_index: u32,
    na: &NormalizedAction,
) -> Result<Decision, &'static str> {
    let to = na
        .tx
        .to
        .and_then(|kind| kind.into_to())
        .ok_or("missing to")?;
    Ok(Decision {
        chain_id,
        action_index,
        action_kind: na_kind_to_copy(na.source),
        to,
        selector: na.selector,
        native_value: na.native_value,
        erc20_amount: na.erc20_amount,
    })
}

fn na_kind_to_copy(k: NormalizedActionKind) -> NormalizedActionKindCopy {
    match k {
        NormalizedActionKind::ContractCall => NormalizedActionKindCopy::ContractCall,
        NormalizedActionKind::RawCall => NormalizedActionKindCopy::RawCall,
        NormalizedActionKind::Erc20Transfer => NormalizedActionKindCopy::Erc20Transfer,
        NormalizedActionKind::Erc20Approve => NormalizedActionKindCopy::Erc20Approve,
        NormalizedActionKind::NativeTransfer => NormalizedActionKindCopy::NativeTransfer,
    }
}

fn action_kind_name(k: NormalizedActionKindCopy) -> &'static str {
    match k {
        NormalizedActionKindCopy::ContractCall => "contract_call",
        NormalizedActionKindCopy::RawCall => "raw_call",
        NormalizedActionKindCopy::Erc20Transfer => "erc20_transfer",
        NormalizedActionKindCopy::Erc20Approve => "erc20_approve",
        NormalizedActionKindCopy::NativeTransfer => "native_transfer",
    }
}

fn selector_hex(selector: Option<[u8; 4]>) -> Option<String> {
    selector.map(|s| format!("0x{}", hex::encode(s)))
}

fn policy_payload(decision: &Decision) -> serde_json::Value {
    serde_json::json!({
        "chain_id": decision.chain_id,
        "action_index": decision.action_index,
        "action_kind": action_kind_name(decision.action_kind),
        "to": decision.to.to_string(),
        "selector": selector_hex(decision.selector),
        "native_value": decision.native_value.to_string(),
        "erc20_amount": decision.erc20_amount.map(|v| v.to_string()),
    })
}

fn simulation_pass_payload(
    return_bytes: &alloy_primitives::Bytes,
    gas_estimate: Option<u64>,
) -> serde_json::Value {
    serde_json::json!({
        "outcome": "pass",
        "return_bytes": format!("0x{}", hex::encode(return_bytes)),
        "gas_estimate": gas_estimate,
    })
}

fn simulation_skipped_payload(decision: &Decision, policy_rule: &str) -> serde_json::Value {
    serde_json::json!({
        "outcome": "skipped",
        "reason": "policy_denied",
        "policy_rule": policy_rule,
        "chain_id": decision.chain_id,
        "action_index": decision.action_index,
        "action_kind": action_kind_name(decision.action_kind),
        "to": decision.to.to_string(),
        "selector": selector_hex(decision.selector),
    })
}

fn simulation_fail_payload(reason: &SimulationFailReason) -> serde_json::Value {
    serde_json::json!({
        "outcome": "fail",
        "fail_reason": simulation_fail_reason(reason),
        "decoded_revert": match reason {
            SimulationFailReason::Revert { decoded } => decoded.clone(),
            SimulationFailReason::Transport | SimulationFailReason::Timeout => None,
        },
    })
}

fn simulation_fail_reason(reason: &SimulationFailReason) -> &'static str {
    match reason {
        SimulationFailReason::Revert { .. } => "revert",
        SimulationFailReason::Transport => "transport",
        SimulationFailReason::Timeout => "timeout",
    }
}

fn simulation_rule(reason: &SimulationFailReason) -> &'static str {
    match reason {
        SimulationFailReason::Revert { .. } => "simulation_revert",
        SimulationFailReason::Transport => "simulation_transport",
        SimulationFailReason::Timeout => "simulation_timeout",
    }
}

fn simulation_detail(reason: &SimulationFailReason) -> String {
    match reason {
        SimulationFailReason::Revert { decoded } => {
            format!(
                "simulation revert: {}",
                decoded.as_deref().unwrap_or("unknown")
            )
        }
        SimulationFailReason::Transport => "simulation transport".to_string(),
        SimulationFailReason::Timeout => "simulation timeout".to_string(),
    }
}
