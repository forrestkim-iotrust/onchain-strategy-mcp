//! Periodic interval worker — fires every `interval_ms` ticks.

use crate::triggers::event::TriggerEvent;
use crate::triggers::worker::TriggerWorker;

pub struct IntervalWorker {
    pub trigger_id: String,
    pub interval_ms: u64,
}

#[async_trait::async_trait]
impl TriggerWorker for IntervalWorker {
    fn kind() -> &'static str {
        "interval"
    }

    async fn run(self: Box<Self>, events: tokio::sync::mpsc::Sender<TriggerEvent>) {
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(self.interval_ms));
        // Skip catch-up ticks when the dispatcher is slow; the dedup window is
        // the agent-facing knob for cadence guarantees.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let payload = serde_json::json!({
                "kind": "interval",
                "ts": chrono::Utc::now().to_rfc3339(),
            });
            let event = TriggerEvent {
                trigger_id: self.trigger_id.clone(),
                payload,
                dedup_key: None,
            };
            match events.try_send(event) {
                Ok(()) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    tracing::warn!(
                        trigger_id = %self.trigger_id,
                        "interval event dropped: dispatcher channel full",
                    );
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    tracing::info!(
                        trigger_id = %self.trigger_id,
                        "interval worker exiting: dispatcher channel closed",
                    );
                    return;
                }
            }
        }
    }
}
