//! Prompt argument schemas — bound in Plan 03 to MCP `prompts/list` entries.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Arguments for the `write_evm_strategy` authoring prompt.")]
pub struct WriteEvmStrategyArgs {
    #[schemars(description = "Free-form intent describing the EVM automation goal.")]
    pub intent: String,
    #[schemars(description = "Optional chain hint (e.g. \"base\", \"mainnet\") to steer prompt output.")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Arguments for the `review_evm_strategy` prompt — points the reviewer at a strategy id.")]
pub struct ReviewEvmStrategyArgs {
    #[schemars(description = "Strategy id whose source should be reviewed.")]
    pub strategy_id: String,
}
