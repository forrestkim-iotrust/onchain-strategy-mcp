//! Strategy tool input schemas — placeholder shapes until Phase 2 persists strategies.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Register a JavaScript strategy (Phase 2 implements persistence).")]
pub struct StrategyRegisterInput {
    #[schemars(description = "Human-readable strategy name; does not need to be unique globally.")]
    pub name: String,
    #[schemars(description = "JavaScript source — executed in a sandbox starting Phase 3.")]
    pub source: String,
    #[schemars(description = "Optional metadata blob persisted alongside the strategy.")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Input referencing a registered strategy by id (Phase 2 fills behaviour).")]
pub struct StrategyIdInput {
    #[schemars(description = "Strategy id returned from `strategy_register`.")]
    pub strategy_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Input for strategy_run_once (Phase 3 wires the JS sandbox).")]
pub struct StrategyRunOnceInput {
    #[schemars(description = "Strategy id to execute once.")]
    pub strategy_id: String,
}
