//! Trigger schema types (v1.2 Trigger Core — Stream A).
//!
//! Shared between `executor-state` (CRUD) and `executor-mcp` (tool inputs).
//!
//! NOTE: This file is canonically owned by Stream A. Stream C is providing it
//! here so the MCP tool surface can compile and be tested independently.
//! When streams merge, the contents of this file are the contract; differences
//! should be resolved in favor of Stream A's version.

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

/// schemars schema generator for free-form JSON object fields (config payloads
/// where the inner shape depends on the trigger kind). Without this, schemars
/// emits `true`/empty schema for `serde_json::Value`, which strict JSON-Schema
/// validators (e.g. Claude Code's MCP client) reject.
fn free_form_object_schema(_: &mut SchemaGenerator) -> Schema {
    serde_json::from_value(serde_json::json!({
        "type": "object",
        "additionalProperties": true,
    }))
    .expect("static free-form object schema")
}

/// Trigger source kinds. v1.2 spike ships `manual` + `interval`; remaining
/// kinds reserve their wire strings so v1.3+ workers can land without
/// schema changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TriggerKind {
    Manual,
    Interval,
    Block,
    Log,
    Mempool,
    Webhook,
}

impl TriggerKind {
    pub fn as_wire(self) -> &'static str {
        match self {
            TriggerKind::Manual => "manual",
            TriggerKind::Interval => "interval",
            TriggerKind::Block => "block",
            TriggerKind::Log => "log",
            TriggerKind::Mempool => "mempool",
            TriggerKind::Webhook => "webhook",
        }
    }

    pub fn from_wire(s: &str) -> Option<Self> {
        Some(match s {
            "manual" => TriggerKind::Manual,
            "interval" => TriggerKind::Interval,
            "block" => TriggerKind::Block,
            "log" => TriggerKind::Log,
            "mempool" => TriggerKind::Mempool,
            "webhook" => TriggerKind::Webhook,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterTriggerInput {
    pub strategy_id: String,
    pub kind: TriggerKind,
    #[schemars(schema_with = "free_form_object_schema")]
    pub config: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_window_ms: Option<u64>,
    /// Free-form natural-language description of what this trigger does
    /// (e.g. `"fires on burner USDC inbound transfer"`). NOT part of the
    /// content hash — purely descriptive so operators can recognise a
    /// trigger without decoding the address / topic blob.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Trigger {
    pub id: String,
    pub strategy_id: String,
    pub kind: TriggerKind,
    /// kind-specific configuration as JSON-encoded string (parsed by workers).
    pub config_json: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicate: Option<String>,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_window_ms: Option<u64>,
    /// v1.6.x: free-form operator-supplied note. See `RegisterTriggerInput::note`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TriggerSummary {
    pub id: String,
    pub strategy_id: String,
    pub kind: TriggerKind,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TriggerEvent {
    pub id: String,
    pub trigger_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_json: Option<String>,
    pub fired_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TriggerListFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<TriggerKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy_id: Option<String>,
}
