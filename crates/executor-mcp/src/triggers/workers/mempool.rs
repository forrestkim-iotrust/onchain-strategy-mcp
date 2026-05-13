//! Mempool trigger worker — subscribes to Alchemy's
//! `alchemy_pendingTransactions` (server-side filtered) and forwards each
//! pending tx as a `TriggerEvent`.
//!
//! Why Alchemy's variant (not standard `newPendingTransactions`)?
//!
//! - Server-side filter on `toAddress` / `fromAddress` — saves bandwidth.
//! - Returns full tx objects (no per-hash `eth_getTransactionByHash` fetch).
//! - Spec: <https://docs.alchemy.com/reference/alchemy-pendingtransactions>
//!
//! Failure model: transient WSS errors are NEVER fatal. The worker loops
//! forever with exponential backoff (1s → 30s, jittered) and re-subscribes
//! on every reconnect. The only exit path is `JoinHandle::abort` (clean
//! cancellation) or the dispatcher channel closing (server shutdown).
//!
//! Backpressure: events are pushed with `try_send`. On `Full`, the event is
//! dropped + warn-logged; the worker NEVER blocks the WSS read loop.

use std::time::Duration;

use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::client::WsConnect;
use alloy_primitives::Address;
use futures_util::StreamExt;

use crate::triggers::event::TriggerEvent;
use crate::triggers::worker::TriggerWorker;

/// Subscribes to Alchemy `alchemy_pendingTransactions` over WSS and forwards
/// each pending tx as a `TriggerEvent`.
pub struct MempoolWorker {
    pub trigger_id: String,
    pub wss_url: String,
    /// Alchemy filter: which `toAddress` values to watch. Server-side
    /// filtered. Empty = ALL pending txs (firehose — use with caution).
    pub to_addresses: Vec<Address>,
    /// Optional `fromAddress` filter (same shape).
    pub from_addresses: Vec<Address>,
}

impl MempoolWorker {
    /// Build the JSON params for the Alchemy subscription. Omits `toAddress`
    /// / `fromAddress` keys when their respective lists are empty so the
    /// subscription matches everything on the unspecified axis.
    fn subscribe_params(&self) -> serde_json::Value {
        let mut filter = serde_json::Map::new();
        if !self.to_addresses.is_empty() {
            let to: Vec<String> = self
                .to_addresses
                .iter()
                .map(|a| format!("{a:?}"))
                .collect();
            filter.insert("toAddress".into(), serde_json::Value::from(to));
        }
        if !self.from_addresses.is_empty() {
            let from: Vec<String> = self
                .from_addresses
                .iter()
                .map(|a| format!("{a:?}"))
                .collect();
            filter.insert("fromAddress".into(), serde_json::Value::from(from));
        }
        // `hashesOnly: false` → server sends full tx objects.
        filter.insert("hashesOnly".into(), serde_json::Value::Bool(false));
        serde_json::json!(["alchemy_pendingTransactions", filter])
    }

    /// One pass: connect, subscribe, drain messages until the connection
    /// drops or the dispatcher channel closes. Returns:
    /// - `Ok(true)`  → channel closed; caller should exit.
    /// - `Ok(false)` → WSS dropped; caller should reconnect.
    /// - `Err(_)`    → setup error; caller should back off and retry.
    async fn run_once(
        &self,
        events: &tokio::sync::mpsc::Sender<TriggerEvent>,
    ) -> anyhow::Result<bool> {
        let ws = WsConnect::new(self.wss_url.clone());
        let provider = ProviderBuilder::new().connect_ws(ws).await?;
        let params = self.subscribe_params();
        let sub = provider
            .subscribe::<serde_json::Value, serde_json::Value>(params)
            .await?;
        let mut stream = sub.into_stream();
        tracing::info!(
            trigger_id = %self.trigger_id,
            to_addresses = self.to_addresses.len(),
            from_addresses = self.from_addresses.len(),
            "mempool worker subscribed",
        );
        while let Some(tx) = stream.next().await {
            // Per Alchemy spec, each push is a full tx object (hashesOnly: false).
            // Use tx.hash as the dedup key — within Alchemy the same hash can
            // arrive twice on a reconnect race.
            let dedup_key = tx
                .get("hash")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            let event = TriggerEvent {
                trigger_id: self.trigger_id.clone(),
                payload: tx,
                dedup_key,
            };
            match events.try_send(event) {
                Ok(()) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    tracing::warn!(
                        trigger_id = %self.trigger_id,
                        "mempool event dropped: dispatcher channel full",
                    );
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    tracing::info!(
                        trigger_id = %self.trigger_id,
                        "mempool worker exiting: dispatcher channel closed",
                    );
                    return Ok(true);
                }
            }
        }
        // Stream ended (server hung up or transport dropped).
        Ok(false)
    }
}

#[async_trait::async_trait]
impl TriggerWorker for MempoolWorker {
    fn kind() -> &'static str {
        "mempool"
    }

    async fn run(self: Box<Self>, events: tokio::sync::mpsc::Sender<TriggerEvent>) {
        // Exponential backoff: 1s → 30s with mild jitter.
        let mut delay_ms: u64 = 1_000;
        loop {
            match self.run_once(&events).await {
                Ok(true) => return,
                Ok(false) => {
                    tracing::warn!(
                        trigger_id = %self.trigger_id,
                        "mempool WSS stream ended; reconnecting",
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        trigger_id = %self.trigger_id,
                        error = %e,
                        "mempool WSS connect/subscribe failed; backing off",
                    );
                }
            }
            // Cheap deterministic jitter — multiply by [0.75, 1.25].
            let jitter_num: u64 =
                75 + (chrono::Utc::now().timestamp_subsec_nanos() as u64 % 51);
            let sleep_ms = (delay_ms.saturating_mul(jitter_num)) / 100;
            tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
            delay_ms = (delay_ms.saturating_mul(2)).min(30_000);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(addr: &str) -> Address {
        addr.parse().unwrap()
    }

    #[test]
    fn subscribe_params_omits_empty_filters() {
        let worker = MempoolWorker {
            trigger_id: "t".into(),
            wss_url: "wss://x".into(),
            to_addresses: vec![],
            from_addresses: vec![],
        };
        let params = worker.subscribe_params();
        let arr = params.as_array().unwrap();
        assert_eq!(arr[0], "alchemy_pendingTransactions");
        let filter = arr[1].as_object().unwrap();
        assert!(!filter.contains_key("toAddress"));
        assert!(!filter.contains_key("fromAddress"));
        assert_eq!(filter.get("hashesOnly"), Some(&serde_json::Value::Bool(false)));
    }

    #[test]
    fn subscribe_params_includes_to_addresses_when_set() {
        let worker = MempoolWorker {
            trigger_id: "t".into(),
            wss_url: "wss://x".into(),
            to_addresses: vec![parse("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913")],
            from_addresses: vec![],
        };
        let params = worker.subscribe_params();
        let filter = params.as_array().unwrap()[1].as_object().unwrap();
        let to = filter.get("toAddress").unwrap().as_array().unwrap();
        assert_eq!(to.len(), 1);
        assert!(to[0].as_str().unwrap().starts_with("0x"));
    }

    #[test]
    fn subscribe_params_includes_from_addresses_when_set() {
        let worker = MempoolWorker {
            trigger_id: "t".into(),
            wss_url: "wss://x".into(),
            to_addresses: vec![],
            from_addresses: vec![parse("0x0000000000000000000000000000000000000001")],
        };
        let params = worker.subscribe_params();
        let filter = params.as_array().unwrap()[1].as_object().unwrap();
        assert!(filter.contains_key("fromAddress"));
        assert!(!filter.contains_key("toAddress"));
    }
}
