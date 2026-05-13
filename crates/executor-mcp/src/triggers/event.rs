//! `TriggerEvent` — one unit of work flowing from a worker to the dispatcher.

/// A single trigger firing. Workers `try_send` these onto the dispatcher
/// channel; on `Full` they MUST drop + warn (do not block the worker loop).
#[derive(Debug, Clone)]
pub struct TriggerEvent {
    pub trigger_id: String,
    pub payload: serde_json::Value,
    pub dedup_key: Option<String>,
}
