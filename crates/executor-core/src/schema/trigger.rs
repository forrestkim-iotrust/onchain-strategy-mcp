//! v1.2 Trigger Core — input + response schemas.
//!
//! Unified event-driven trigger model. All trigger sources (manual, interval,
//! block, log, mempool, webhook) share one schema, one MCP API, one worker
//! trait, one sandbox extension. The same shapes serve `executor-state` CRUD
//! and `executor-mcp` tool I/O.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Kind of trigger source. Wire format is lowercase string.
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
    pub fn as_str(self) -> &'static str {
        match self {
            TriggerKind::Manual => "manual",
            TriggerKind::Interval => "interval",
            TriggerKind::Block => "block",
            TriggerKind::Log => "log",
            TriggerKind::Mempool => "mempool",
            TriggerKind::Webhook => "webhook",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
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
#[schemars(description = "Register a trigger for a strategy (content-addressed; idempotent on same input).")]
pub struct RegisterTriggerInput {
    #[schemars(description = "Strategy id this trigger fires.")]
    pub strategy_id: String,
    #[schemars(description = "Trigger source kind: manual|interval|block|log|mempool|webhook.")]
    pub kind: TriggerKind,
    #[schemars(description = "Kind-specific JSON config (e.g. {\"interval_ms\":1000}).")]
    pub config: serde_json::Value,
    #[schemars(description = "Optional `(event) => bool` JS predicate source.")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicate: Option<String>,
    #[schemars(description = "Optional dedup window in milliseconds. null/0 disables dedup.")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_window_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Full trigger row.")]
pub struct Trigger {
    pub id: String,
    pub strategy_id: String,
    pub kind: TriggerKind,
    pub config_json: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicate: Option<String>,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_window_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "List-view trigger row (no `config_json` / `predicate`).")]
pub struct TriggerSummary {
    pub id: String,
    pub strategy_id: String,
    pub kind: TriggerKind,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_window_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Single trigger fire event.")]
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
#[schemars(description = "Optional filter for trigger_list.")]
pub struct TriggerListFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<TriggerKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy_id: Option<String>,
}
