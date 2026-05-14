//! `WorkerPool` — owns the per-trigger `JoinHandle` table.

use std::collections::HashMap;

use alloy_primitives::Address;
use executor_core::schema::trigger::{Trigger, TriggerKind};

use crate::triggers::event::TriggerEvent;
use crate::triggers::worker::TriggerWorker;
use crate::triggers::workers::interval::IntervalWorker;
use crate::triggers::workers::log::{LogWorker, parse_log_config};
use crate::triggers::workers::mempool::MempoolWorker;

pub struct WorkerPool {
    handles: HashMap<String, tokio::task::JoinHandle<()>>,
}

impl Default for WorkerPool {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkerPool {
    pub fn new() -> Self {
        Self {
            handles: HashMap::new(),
        }
    }

    /// Spawn the background worker for `trigger`, if its kind has one.
    ///
    /// `mempool_wss_url` is the shared `[trigger].mempool_wss_url` setting;
    /// `kind = mempool` workers are skipped (warn-logged) when it is `None`.
    pub fn spawn(
        &mut self,
        trigger: &Trigger,
        events: tokio::sync::mpsc::Sender<TriggerEvent>,
        mempool_wss_url: &Option<String>,
    ) -> anyhow::Result<()> {
        match trigger.kind {
            TriggerKind::Interval => {
                let config: serde_json::Value =
                    serde_json::from_str(&trigger.config_json).map_err(|e| {
                        anyhow::anyhow!(
                            "interval trigger {} has invalid config_json: {e}",
                            trigger.id
                        )
                    })?;
                let interval_ms = config
                    .get("interval_ms")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "interval trigger {} missing config.interval_ms",
                            trigger.id
                        )
                    })?;
                if interval_ms == 0 {
                    return Err(anyhow::anyhow!(
                        "interval trigger {} has zero interval_ms",
                        trigger.id
                    ));
                }
                let worker = Box::new(IntervalWorker {
                    trigger_id: trigger.id.clone(),
                    interval_ms,
                });
                let handle = tokio::spawn(worker.run(events));
                self.handles.insert(trigger.id.clone(), handle);
                Ok(())
            }
            TriggerKind::Manual => {
                // No background loop — driven by MCP-tool synthesis.
                Ok(())
            }
            TriggerKind::Mempool => {
                let Some(wss_url) = mempool_wss_url.as_deref() else {
                    tracing::warn!(
                        trigger_id = %trigger.id,
                        "mempool trigger skipped: [trigger].mempool_wss_url not configured",
                    );
                    return Ok(());
                };
                let (to_addresses, from_addresses) =
                    parse_mempool_config(&trigger.id, &trigger.config_json)?;
                let worker = Box::new(MempoolWorker {
                    trigger_id: trigger.id.clone(),
                    wss_url: wss_url.to_string(),
                    to_addresses,
                    from_addresses,
                });
                let handle = tokio::spawn(worker.run(events));
                self.handles.insert(trigger.id.clone(), handle);
                Ok(())
            }
            TriggerKind::Log => {
                let Some(wss_url) = mempool_wss_url.as_deref() else {
                    tracing::warn!(
                        trigger_id = %trigger.id,
                        "log trigger skipped: [trigger].mempool_wss_url not configured",
                    );
                    return Ok(());
                };
                let (address, topics) = parse_log_config(&trigger.id, &trigger.config_json)?;
                let worker = Box::new(LogWorker {
                    trigger_id: trigger.id.clone(),
                    wss_url: wss_url.to_string(),
                    address,
                    topics,
                });
                let handle = tokio::spawn(worker.run(events));
                self.handles.insert(trigger.id.clone(), handle);
                Ok(())
            }
            TriggerKind::Block | TriggerKind::Webhook => {
                tracing::warn!(
                    trigger_id = %trigger.id,
                    kind = trigger.kind.as_wire(),
                    "trigger kind not yet implemented; skipping spawn",
                );
                Ok(())
            }
        }
    }

    /// Returns true if a worker handle is currently tracked for `trigger_id`.
    /// Test-helper used by per-worker spawn-skip unit tests.
    #[cfg(test)]
    pub(crate) fn has_handle(&self, trigger_id: &str) -> bool {
        self.handles.contains_key(trigger_id)
    }

    pub fn stop(&mut self, trigger_id: &str) {
        if let Some(handle) = self.handles.remove(trigger_id) {
            handle.abort();
        }
    }
}

/// Parse `{ to_address?: [hex,...], from_address?: [hex,...] }` from a
/// trigger's `config_json`. Both arrays are optional and default to empty.
///
/// Pure helper so the parse paths are testable without spinning a runtime.
pub(crate) fn parse_mempool_config(
    trigger_id: &str,
    config_json: &str,
) -> anyhow::Result<(Vec<Address>, Vec<Address>)> {
    let config: serde_json::Value = serde_json::from_str(config_json).map_err(|e| {
        anyhow::anyhow!(
            "mempool trigger {trigger_id} has invalid config_json: {e}"
        )
    })?;
    let to_addresses = parse_address_array(trigger_id, &config, "to_address")?;
    let from_addresses = parse_address_array(trigger_id, &config, "from_address")?;
    Ok((to_addresses, from_addresses))
}

fn parse_address_array(
    trigger_id: &str,
    config: &serde_json::Value,
    field: &str,
) -> anyhow::Result<Vec<Address>> {
    let Some(value) = config.get(field) else {
        return Ok(Vec::new());
    };
    let arr = value.as_array().ok_or_else(|| {
        anyhow::anyhow!(
            "mempool trigger {trigger_id} config.{field} must be an array of hex addresses"
        )
    })?;
    let mut out = Vec::with_capacity(arr.len());
    for entry in arr {
        let s = entry.as_str().ok_or_else(|| {
            anyhow::anyhow!(
                "mempool trigger {trigger_id} config.{field} entries must be hex strings"
            )
        })?;
        let addr: Address = s.parse().map_err(|e| {
            anyhow::anyhow!(
                "mempool trigger {trigger_id} config.{field} has invalid address `{s}`: {e}"
            )
        })?;
        out.push(addr);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use executor_core::schema::trigger::TriggerKind;

    fn make_trigger(kind: TriggerKind, config_json: &str) -> Trigger {
        Trigger {
            id: "trig_test".into(),
            strategy_id: "strat_test".into(),
            kind,
            config_json: config_json.into(),
            predicate: None,
            dedup_window_ms: None,
            enabled: true,
            last_fired_at: None,
            created_at: "1970-01-01T00:00:00Z".into(),
            note: None,
        }
    }

    #[test]
    fn mempool_config_parses_to_address_array() {
        let cfg = r#"{"to_address":["0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"]}"#;
        let (to, from) = parse_mempool_config("t", cfg).unwrap();
        assert_eq!(to.len(), 1);
        assert!(from.is_empty());
    }

    #[test]
    fn mempool_config_parses_both_filters() {
        let cfg = r#"{
            "to_address":["0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"],
            "from_address":["0x0000000000000000000000000000000000000001"]
        }"#;
        let (to, from) = parse_mempool_config("t", cfg).unwrap();
        assert_eq!(to.len(), 1);
        assert_eq!(from.len(), 1);
    }

    #[test]
    fn mempool_config_rejects_bad_hex() {
        let cfg = r#"{"to_address":["not-a-hex-address"]}"#;
        let err = parse_mempool_config("t", cfg).unwrap_err();
        assert!(err.to_string().contains("invalid address"));
    }

    #[test]
    fn mempool_config_defaults_to_empty_when_omitted() {
        let (to, from) = parse_mempool_config("t", "{}").unwrap();
        assert!(to.is_empty());
        assert!(from.is_empty());
    }

    #[tokio::test]
    async fn mempool_pool_spawn_skips_when_wss_url_none() {
        let mut pool = WorkerPool::new();
        let trigger = make_trigger(TriggerKind::Mempool, r#"{"to_address":[]}"#);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        // Must NOT error, must NOT spawn a worker.
        pool.spawn(&trigger, tx, &None).unwrap();
        assert!(pool.handles.is_empty());
    }
}
