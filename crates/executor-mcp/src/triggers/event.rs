//! Shared trigger-event type — what every worker sends into the dispatcher.

use serde_json::Value;

/// One fire from a trigger source. The dispatcher consumes a stream of these
/// from a shared `mpsc::Receiver`. All worker kinds emit the same shape so
/// downstream code is uniform.
#[derive(Debug, Clone)]
pub struct TriggerEvent {
    /// Trigger id (ULID) this event belongs to. Used by the dispatcher to
    /// look up the live `Trigger` row (predicate, dedup window, strategy id).
    pub trigger_id: String,
    /// Kind-specific payload. Becomes `ctx.event` inside the strategy
    /// sandbox (Stream B). Workers serialise their own shape.
    pub payload: Value,
    /// Optional dedup key. When `Some`, the dispatcher consults
    /// `state.check_dedup(trigger_id, key, window_ms)` before firing.
    pub dedup_key: Option<String>,
}
