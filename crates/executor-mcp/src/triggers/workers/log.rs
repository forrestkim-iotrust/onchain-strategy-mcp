//! Log subscription trigger worker — subscribes to `eth_subscribe("logs", …)`
//! over WSS and forwards each emitted log as a `TriggerEvent`.
//!
//! Unlike the mempool worker (which catches *pending* txs by `to` / `from`
//! address), this worker fires on logs *included* in blocks. That makes it
//! the right primitive for "watch incoming ERC20 transfers to the burner":
//! a USDC transfer has `tx.to = USDC contract`, so a mempool `toAddress`
//! filter would miss it — but the `Transfer` log's `topic2` (indexed `to`)
//! catches it cleanly.
//!
//! Filter shape (canonical `eth_subscribe("logs", …)`):
//! - `address` — optional contract address.
//! - `topics`  — up to 4 slots (`topic0..topic3`). `None` in a slot means
//!   "wildcard" (any value); `Some(B256)` pins that slot.
//!
//! Failure / backpressure model mirrors `MempoolWorker`: WSS errors loop
//! forever with exponential backoff (1s → 30s, jittered); events are pushed
//! with `try_send`. `removed: true` logs (reorg) are emitted as normal —
//! predicate filtering is the agent's escape hatch.
//!
//! Spec: <https://docs.alchemy.com/reference/logs> (canonical Ethereum).

use std::time::Duration;

use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::client::WsConnect;
use alloy::rpc::types::Filter;
use alloy_primitives::{Address, B256};
use futures_util::StreamExt;

use crate::triggers::event::TriggerEvent;
use crate::triggers::worker::TriggerWorker;

/// Subscribes to `eth_subscribe("logs", filter)` over WSS and forwards each
/// matched log as a `TriggerEvent`.
pub struct LogWorker {
    pub trigger_id: String,
    pub wss_url: String,
    /// Optional contract address to restrict the subscription to.
    pub address: Option<Address>,
    /// Up to 4 topic slots. `None` in a slot is a wildcard; `Some(h)` pins
    /// that slot. Trailing `None`s are dropped on the wire.
    pub topics: Vec<Option<B256>>,
}

impl LogWorker {
    /// Build the alloy `Filter` for this worker.
    fn build_filter(&self) -> Filter {
        let mut filter = Filter::new();
        if let Some(addr) = self.address {
            filter = filter.address(addr);
        }
        // Apply per-slot topic pins. alloy's `Filter::event_signature` / `topicN`
        // helpers each take an `Into<FilterSet<B256>>`; passing the B256
        // directly pins the slot.
        if let Some(Some(t0)) = self.topics.first() {
            filter = filter.event_signature(*t0);
        }
        if let Some(Some(t1)) = self.topics.get(1) {
            filter = filter.topic1(*t1);
        }
        if let Some(Some(t2)) = self.topics.get(2) {
            filter = filter.topic2(*t2);
        }
        if let Some(Some(t3)) = self.topics.get(3) {
            filter = filter.topic3(*t3);
        }
        filter
    }

    /// One pass: connect, subscribe, drain logs until the connection drops
    /// or the dispatcher channel closes. Returns:
    /// - `Ok(true)`  → channel closed; caller should exit.
    /// - `Ok(false)` → WSS dropped; caller should reconnect.
    /// - `Err(_)`    → setup error; caller should back off and retry.
    async fn run_once(
        &self,
        events: &tokio::sync::mpsc::Sender<TriggerEvent>,
    ) -> anyhow::Result<bool> {
        let ws = WsConnect::new(self.wss_url.clone());
        let provider = ProviderBuilder::new().connect_ws(ws).await?;
        let filter = self.build_filter();
        let sub = provider.subscribe_logs(&filter).await?;
        let mut stream = sub.into_stream();
        tracing::info!(
            trigger_id = %self.trigger_id,
            address = ?self.address,
            topic_slots = self.topics.len(),
            "log worker subscribed",
        );
        while let Some(log) = stream.next().await {
            // Dedup key = "{tx_hash}:{log_index}" when both present. Reorged
            // logs (`removed: true`) are emitted as well — the predicate is
            // the agent's escape hatch for reorg-sensitive flows.
            let dedup_key = match (log.transaction_hash, log.log_index) {
                (Some(h), Some(i)) => Some(format!("{h:?}:{i}")),
                _ => None,
            };
            // Serialize via alloy's canonical Log serde. Failure here is
            // *not* worker-fatal; skip the event.
            let payload = match serde_json::to_value(&log) {
                Ok(v) => v,
                Err(err) => {
                    tracing::warn!(
                        trigger_id = %self.trigger_id,
                        error = %err,
                        "log serialization failed; dropping event",
                    );
                    continue;
                }
            };
            let event = TriggerEvent {
                trigger_id: self.trigger_id.clone(),
                payload,
                dedup_key,
            };
            match events.try_send(event) {
                Ok(()) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    tracing::warn!(
                        trigger_id = %self.trigger_id,
                        "log event dropped: dispatcher channel full",
                    );
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    tracing::info!(
                        trigger_id = %self.trigger_id,
                        "log worker exiting: dispatcher channel closed",
                    );
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

#[async_trait::async_trait]
impl TriggerWorker for LogWorker {
    fn kind() -> &'static str {
        "log"
    }

    async fn run(self: Box<Self>, events: tokio::sync::mpsc::Sender<TriggerEvent>) {
        let mut delay_ms: u64 = 1_000;
        loop {
            match self.run_once(&events).await {
                Ok(true) => return,
                Ok(false) => {
                    tracing::warn!(
                        trigger_id = %self.trigger_id,
                        "log WSS stream ended; reconnecting",
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        trigger_id = %self.trigger_id,
                        error = %e,
                        "log WSS connect/subscribe failed; backing off",
                    );
                }
            }
            // Deterministic jitter ∈ [0.75, 1.25] of `delay_ms`, additive
            // ≤250ms cap matches the design contract.
            let jitter_num: u64 =
                75 + (chrono::Utc::now().timestamp_subsec_nanos() as u64 % 51);
            let sleep_ms = (delay_ms.saturating_mul(jitter_num)) / 100;
            tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
            delay_ms = (delay_ms.saturating_mul(2)).min(30_000);
        }
    }
}

/// Parse `{ address?: hex, topics?: [hex|null, …≤4] }` out of a log trigger's
/// `config_json`. Both fields optional; topics array is bounded to 4 entries.
///
/// Pure helper so the parse paths are testable without a tokio runtime.
pub(crate) fn parse_log_config(
    trigger_id: &str,
    config_json: &str,
) -> anyhow::Result<(Option<Address>, Vec<Option<B256>>)> {
    let config: serde_json::Value = serde_json::from_str(config_json).map_err(|e| {
        anyhow::anyhow!("log trigger {trigger_id} has invalid config_json: {e}")
    })?;

    let address = match config.get("address") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => Some(s.parse::<Address>().map_err(|e| {
            anyhow::anyhow!(
                "log trigger {trigger_id} config.address has invalid hex `{s}`: {e}"
            )
        })?),
        Some(_) => {
            return Err(anyhow::anyhow!(
                "log trigger {trigger_id} config.address must be a hex string or null"
            ));
        }
    };

    let topics = match config.get("topics") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(serde_json::Value::Array(arr)) => {
            if arr.len() > 4 {
                return Err(anyhow::anyhow!(
                    "log trigger {trigger_id} config.topics has {} entries (max 4)",
                    arr.len()
                ));
            }
            let mut out = Vec::with_capacity(arr.len());
            for (idx, entry) in arr.iter().enumerate() {
                match entry {
                    serde_json::Value::Null => out.push(None),
                    serde_json::Value::String(s) => {
                        let h: B256 = s.parse().map_err(|e| {
                            anyhow::anyhow!(
                                "log trigger {trigger_id} config.topics[{idx}] invalid hex `{s}`: {e}"
                            )
                        })?;
                        out.push(Some(h));
                    }
                    _ => {
                        return Err(anyhow::anyhow!(
                            "log trigger {trigger_id} config.topics[{idx}] must be hex string or null"
                        ));
                    }
                }
            }
            out
        }
        Some(_) => {
            return Err(anyhow::anyhow!(
                "log trigger {trigger_id} config.topics must be an array (or omitted)"
            ));
        }
    };

    Ok((address, topics))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::triggers::pool::WorkerPool;
    use executor_core::schema::trigger::{Trigger, TriggerKind};

    fn make_trigger(config_json: &str) -> Trigger {
        Trigger {
            id: "trig_log_test".into(),
            strategy_id: "strat_test".into(),
            kind: TriggerKind::Log,
            config_json: config_json.into(),
            predicate: None,
            dedup_window_ms: None,
            enabled: true,
            last_fired_at: None,
            created_at: "1970-01-01T00:00:00Z".into(),
            strategy_lineage_id: None,
        }
    }

    #[test]
    fn log_config_parses_address_only() {
        let cfg = r#"{"address":"0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"}"#;
        let (addr, topics) = parse_log_config("t", cfg).unwrap();
        assert!(addr.is_some());
        assert!(topics.is_empty());
    }

    #[test]
    fn log_config_parses_address_and_topics_with_nulls() {
        // Transfer(address,address,uint256) sig + null `from` + pinned `to`.
        let cfg = r#"{
            "address":"0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
            "topics":[
                "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
                null,
                "0x0000000000000000000000000000000000000000000000000000000000000abc"
            ]
        }"#;
        let (addr, topics) = parse_log_config("t", cfg).unwrap();
        assert!(addr.is_some());
        assert_eq!(topics.len(), 3);
        assert!(topics[0].is_some());
        assert!(topics[1].is_none());
        assert!(topics[2].is_some());
    }

    #[test]
    fn log_config_rejects_more_than_4_topics() {
        let zero = "0x0000000000000000000000000000000000000000000000000000000000000000";
        let cfg = format!(
            r#"{{"topics":["{zero}","{zero}","{zero}","{zero}","{zero}"]}}"#
        );
        let err = parse_log_config("t", &cfg).unwrap_err();
        assert!(err.to_string().contains("max 4"), "got: {err}");
    }

    #[test]
    fn log_config_rejects_invalid_hex_address() {
        let cfg = r#"{"address":"not-a-real-address"}"#;
        let err = parse_log_config("t", cfg).unwrap_err();
        assert!(err.to_string().contains("invalid hex"), "got: {err}");
    }

    #[tokio::test]
    async fn log_pool_spawn_skips_when_wss_url_none() {
        let mut pool = WorkerPool::new();
        let trigger = make_trigger(r#"{"address":"0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"}"#);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        pool.spawn(&trigger, tx, &None).unwrap();
        // No handle should have been inserted.
        assert!(!pool.has_handle(&trigger.id));
    }
}
