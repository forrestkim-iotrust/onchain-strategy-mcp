//! Strategy tool input + response schemas.
//!
//! Phase 2 splits the previous `metadata: Option<Value>` into top-level
//! `description: Option<String>` + `tags: Option<Vec<String>>` (D-07a /
//! RESEARCH Open Q4 option B) and adds the response types the MCP layer
//! serializes in Plan 02-02.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(description = "Register a JavaScript strategy (content-addressed; idempotent on same source).")]
pub struct StrategyRegisterInput {
    #[schemars(description = "Human-readable name; UNIQUE among non-deleted strategies.")]
    pub name: String,
    #[schemars(description = "JavaScript source — executed in a sandbox starting Phase 3. Max 256 KiB.")]
    pub source: String,
    #[schemars(description = "Optional free-form description (max 4096 chars).")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[schemars(description = "Optional tags (max 16 items, each max 64 chars).")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Input referencing a registered strategy by id (used by strategy_delete).")]
pub struct StrategyIdInput {
    #[schemars(description = "Strategy id returned from `strategy_register` (lower-case hex SHA-256, 64 chars).")]
    pub strategy_id: String,
}

/// Phase-3 input for the `strategy_run` MCP tool. Replaces the Phase-1
/// `StrategyRunOnceInput` placeholder; the alias below preserves the old
/// name for one phase to soften the rename.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(description = "Execute a registered JavaScript strategy once in a sandbox.")]
pub struct StrategyRunInput {
    #[schemars(description = "Strategy id (lower-case hex SHA-256, 64 chars).")]
    pub strategy_id: String,
}

/// Deprecated alias preserved for one phase. Phase 4 may delete it.
#[deprecated(note = "Use `StrategyRunInput` instead. The `_once` qualifier was a Phase-1 placeholder.")]
pub use StrategyRunInput as StrategyRunOnceInput;

/// XOR input for `strategy_get`: agent supplies either the content-addressed
/// `strategy_id` or the human-friendly `name` (active strategies only).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
#[schemars(extend("type" = "object"))]
pub enum StrategyGetInput {
    ById { strategy_id: String },
    ByName { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Response for strategy_register (D-07).")]
pub struct StrategyRegisterResponse {
    pub strategy_id: String,
    pub name: String,
    pub created_at: String,
    pub already_exists: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_tags: Option<Vec<String>>,
    /// Surfaced when the existing row is soft-deleted (Pitfall 9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "List item — note: `source` is intentionally absent (D-07a).")]
pub struct StrategyListItem {
    pub strategy_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Response for strategy_list (D-07a).")]
pub struct StrategyListResponse {
    pub strategies: Vec<StrategyListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Response for strategy_get (D-07b) — includes source.")]
pub struct StrategyGetResponse {
    pub strategy_id: String,
    pub name: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Response for strategy_delete (D-07c) — idempotent.")]
pub struct StrategyDeleteResponse {
    pub strategy_id: String,
    pub deleted_at: String,
}
