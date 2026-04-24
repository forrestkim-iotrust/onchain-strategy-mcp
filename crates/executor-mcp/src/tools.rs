//! `#[tool_router]` impl block declaring the 8 Phase 1 tools.
//!
//! Phase map (cf. PLAN `unimplemented_phase_map`):
//!   - strategy_register → 2  (STR-01, Phase 2: Strategy State and Journal)
//!   - strategy_delete   → 2  (STR-02, Phase 2)
//!   - strategy_run_once → 6  (runtime loop — simulate / policy / sign / broadcast)
//!   - policy_update     → 5  (POL-01..06, Phase 5)
//!
//! Read-only tools (`strategy_list`, `strategy_get`, `execution_get`,
//! `policy_get`) return placeholder shapes today; Phase 2+ fills behaviour.

use executor_core::schema::{
    execution::ExecutionIdInput,
    policy::PolicyUpdateInput,
    strategy::{StrategyIdInput, StrategyRegisterInput, StrategyRunOnceInput},
};
use rmcp::{
    ErrorData as McpError,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content},
    tool, tool_router,
};

use crate::errors::unimplemented_err;
use crate::server::ExecutorServer;

#[tool_router(vis = "pub(crate)")]
impl ExecutorServer {
    // ─────────── WRITE-CAPABLE TOOLS (Unimplemented in Phase 1) ───────────

    #[tool(
        name = "strategy_register",
        description = "Register a JavaScript strategy. NOT YET IMPLEMENTED — lands in Phase 2."
    )]
    async fn strategy_register(
        &self,
        Parameters(_input): Parameters<StrategyRegisterInput>,
    ) -> Result<CallToolResult, McpError> {
        Err(unimplemented_err("strategy_register", 2))
    }

    #[tool(
        name = "strategy_delete",
        description = "Delete a registered strategy. NOT YET IMPLEMENTED — lands in Phase 2."
    )]
    async fn strategy_delete(
        &self,
        Parameters(_input): Parameters<StrategyIdInput>,
    ) -> Result<CallToolResult, McpError> {
        Err(unimplemented_err("strategy_delete", 2))
    }

    #[tool(
        name = "strategy_run_once",
        description = "Execute a strategy once (simulate → policy → sign → broadcast). NOT YET IMPLEMENTED — lands in Phase 6."
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

    // ─────────── READ-ONLY TOOLS (placeholder shapes) ───────────

    #[tool(
        name = "strategy_list",
        description = "List registered strategies. Returns an empty array until Phase 2 persists state."
    )]
    async fn strategy_list(&self) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![Content::text("[]")]))
    }

    #[tool(
        name = "strategy_get",
        description = "Get a strategy by id. Returns resource_not_found until Phase 2 persists state."
    )]
    async fn strategy_get(
        &self,
        Parameters(_input): Parameters<StrategyIdInput>,
    ) -> Result<CallToolResult, McpError> {
        Err(McpError::resource_not_found(
            "strategy not found (empty store)",
            Some(serde_json::json!({
                "code": "not_found",
                "tool": "strategy_get",
                "phase": 2,
            })),
        ))
    }

    #[tool(
        name = "execution_get",
        description = "Get an execution report by id. Returns resource_not_found until Phase 6 records executions."
    )]
    async fn execution_get(
        &self,
        Parameters(_input): Parameters<ExecutionIdInput>,
    ) -> Result<CallToolResult, McpError> {
        Err(McpError::resource_not_found(
            "execution not found (empty store)",
            Some(serde_json::json!({
                "code": "not_found",
                "tool": "execution_get",
                "phase": 6,
            })),
        ))
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
