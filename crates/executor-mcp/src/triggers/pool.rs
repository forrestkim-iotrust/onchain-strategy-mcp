//! `WorkerPool` — owns the per-trigger `JoinHandle` table.

use std::collections::HashMap;

use executor_core::schema::trigger::{Trigger, TriggerKind};

use crate::triggers::event::TriggerEvent;
use crate::triggers::worker::TriggerWorker;
use crate::triggers::workers::interval::IntervalWorker;

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

    pub fn spawn(
        &mut self,
        trigger: &Trigger,
        events: tokio::sync::mpsc::Sender<TriggerEvent>,
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
            TriggerKind::Block
            | TriggerKind::Log
            | TriggerKind::Mempool
            | TriggerKind::Webhook => {
                tracing::warn!(
                    trigger_id = %trigger.id,
                    kind = trigger.kind.as_wire(),
                    "trigger kind not yet implemented; skipping spawn",
                );
                Ok(())
            }
        }
    }

    pub fn stop(&mut self, trigger_id: &str) {
        if let Some(handle) = self.handles.remove(trigger_id) {
            handle.abort();
        }
    }
}
