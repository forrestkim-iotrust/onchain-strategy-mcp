//! Interval worker — periodic `tokio::time::interval` firing one
//! `TriggerEvent` per tick. Missed ticks are skipped (catching up would
//! produce a thundering herd after pauses).

use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio::time::{MissedTickBehavior, interval};

use crate::triggers::event::TriggerEvent;
use crate::triggers::worker::TriggerWorker;

/// Spec: see [`crate::triggers::worker::TriggerWorker`].
pub struct IntervalWorker {
    pub trigger_id: String,
    pub interval_ms: u64,
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[async_trait]
impl TriggerWorker for IntervalWorker {
    fn kind() -> &'static str {
        "interval"
    }

    async fn run(self: Box<Self>, events: mpsc::Sender<TriggerEvent>) {
        // Guard against pathological configs — sub-millisecond intervals
        // would busy-loop. Floor at 1ms; the dispatcher applies its own
        // backpressure via channel capacity.
        let period = Duration::from_millis(self.interval_ms.max(1));
        let mut ticker = interval(period);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        // First tick fires immediately; consume it so callers see an actual
        // `interval_ms` cadence rather than an extra burst at t=0.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let evt = TriggerEvent {
                trigger_id: self.trigger_id.clone(),
                payload: serde_json::json!({
                    "kind": "interval",
                    "ts": now_rfc3339(),
                }),
                dedup_key: None,
            };
            // Backpressure: try_send so a saturated dispatcher doesn't block
            // the worker. Drop + warn on Full (1024 capacity in practice).
            match events.try_send(evt) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    tracing::warn!(
                        trigger_id = %self.trigger_id,
                        "interval worker: dispatcher channel full — dropping tick",
                    );
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    tracing::info!(
                        trigger_id = %self.trigger_id,
                        "interval worker: dispatcher channel closed — exiting",
                    );
                    return;
                }
            }
        }
    }
}
