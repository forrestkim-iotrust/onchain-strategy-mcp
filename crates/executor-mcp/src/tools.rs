//! `#[tool_router]` impl block — 8 MCP tools.
//!
//! Phase transition map (Phase 2 delivers the first two groups):
//!   - Real storage-backed: strategy_register, strategy_list, strategy_get,
//!     strategy_delete, execution_get (returns not_found until runs are
//!     inserted — Plan 02-03 wires `strategy_run_once` to start emitting runs).
//!   - Still placeholders: strategy_run_once (Phase 6), policy_get (Phase 5,
//!     returns empty shape), policy_update (Phase 5, unimplemented_err).

use executor_core::schema::{
    execution::{ExecutionGetResponse, ExecutionIdInput},
    policy::PolicyUpdateInput,
    strategy::{
        StrategyDeleteResponse, StrategyGetInput, StrategyGetResponse, StrategyIdInput,
        StrategyListItem, StrategyListResponse, StrategyRegisterInput, StrategyRegisterResponse,
        StrategyRunOnceInput,
    },
};
use executor_state::{RegisterOutcome, StateError, Strategy, StrategySummary};
use rmcp::{
    ErrorData as McpError,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content},
    tool, tool_router,
};
use serde::{Deserialize, Serialize};

use crate::{
    errors::{invalid_params, map_state_error, storage_error, unimplemented_err},
    server::ExecutorServer,
    validation::{validate_register, validate_strategy_id_format},
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
        description = "Get a run by id. Returns not_found until Phase 3 begins inserting runs via strategy_run_once."
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
        name = "strategy_run_once",
        description = "Execute a strategy once. NOT YET IMPLEMENTED — lands in Phase 6."
    )]
    async fn strategy_run_once(
        &self,
        Parameters(_input): Parameters<StrategyRunOnceInput>,
    ) -> Result<CallToolResult, McpError> {
        Err(unimplemented_err("strategy_run_once", 6))
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
        description = "Get the current policy. Returns a placeholder shape until Phase 5 implements the policy engine."
    )]
    async fn policy_get(&self) -> Result<CallToolResult, McpError> {
        let placeholder = serde_json::json!({
            "chains": [],
            "targets": [],
            "selectors": [],
            "note": "policy engine lands in Phase 5",
        });
        Ok(CallToolResult::success(vec![Content::text(
            placeholder.to_string(),
        )]))
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
