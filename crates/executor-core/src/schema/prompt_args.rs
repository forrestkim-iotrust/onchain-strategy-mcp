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

/// v1.4 Track E1: `safety_review` accepts a proposed strategy source string
/// (not a registered id) — the prompt body inlines static-analysis of that
/// source against the loaded policy before the agent calls `strategy_register`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(
    description = "Arguments for the `safety_review` prompt — vets a proposed strategy source before register."
)]
pub struct SafetyReviewArgs {
    #[schemars(description = "Proposed strategy source (JS) to evaluate against the loaded policy.")]
    pub source: String,
}

/// v1.4 Track E1: `author_strategy` accepts free-text intent and returns a
/// bundle skeleton + the most relevant embedded example.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(
    description = "Arguments for the `author_strategy` prompt — bundle-shaped authoring guide."
)]
pub struct AuthorStrategyArgs {
    #[schemars(description = "Free-text user intent (e.g. \"ETH→USDC→Aave funnel\").")]
    pub intent: String,
}

/// v1.11 Track E2: `triage_run` accepts a run id and composes execution +
/// journal + receipts + policy decisions into a structured forensics report.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(
    description = "Arguments for the `triage_run` prompt — composes a run-forensics report."
)]
pub struct TriageRunArgs {
    #[schemars(description = "Run id (26-char Crockford ULID) to triage.")]
    pub run_id: String,
}
