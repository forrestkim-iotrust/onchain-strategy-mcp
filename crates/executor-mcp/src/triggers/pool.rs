//! `WorkerPool` — owns spawned worker `JoinHandle`s keyed by trigger id.
//!
//! Lifecycle helpers (`spawn`, `stop`, `restart`) are called from
//! `ExecutorServer::from_config` at boot and from the `trigger_*` MCP tools
//! whenever an operator enables / disables / deletes a trigger.

use std::collections::HashMap;

use anyhow::Result;
use executor_core::schema::trigger::{Trigger, TriggerKind};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::event::TriggerEvent;
use super::worker::TriggerWorker;
use super::workers::interval::IntervalWorker;

#[derive(Default)]
pub struct WorkerPool {
    handles: HashMap<String, JoinHandle<()>>,
}

impl WorkerPool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawn the matching worker for `trigger.kind`. No-op for kinds that
    /// don't yet have a worker implementation (logs a warn and returns Ok so
    /// boot keeps going).
    pub fn spawn(&mut self, trigger: &Trigger, events: mpsc::Sender<TriggerEvent>) -> Result<()> {
        // Idempotent: an already-spawned worker for this id stays as-is. Use
        // `restart` to swap configs.
        if self.handles.contains_key(&trigger.id) {
            return Ok(());
        }
        match trigger.kind {
            TriggerKind::Interval => {
                let interval_ms = parse_interval_ms(&trigger.config_json).unwrap_or_else(|e| {
                    tracing::warn!(
                        trigger_id = %trigger.id,
                        error = %e,
                        "interval config parse failed — defaulting to 1000ms",
                    );
                    1000
                });
                let worker = Box::new(IntervalWorker {
                    trigger_id: trigger.id.clone(),
                    interval_ms,
                });
                let handle = tokio::spawn(async move {
                    <IntervalWorker as TriggerWorker>::run(worker, events).await;
                });
                self.handles.insert(trigger.id.clone(), handle);
                tracing::info!(trigger_id = %trigger.id, interval_ms, "interval worker spawned");
            }
            TriggerKind::Manual => {
                // No background worker — manual triggers fire via the MCP
                // `strategy_run` tool injecting events directly into the
                // dispatcher channel.
                tracing::debug!(trigger_id = %trigger.id, "manual trigger: no worker to spawn");
            }
            TriggerKind::Block
            | TriggerKind::Log
            | TriggerKind::Mempool
            | TriggerKind::Webhook => {
                tracing::warn!(
                    trigger_id = %trigger.id,
                    kind = trigger.kind.as_str(),
                    "trigger kind not yet implemented — skipping worker spawn",
                );
            }
        }
        Ok(())
    }

    /// Abort the worker for `trigger_id` (if any) and forget it.
    pub fn stop(&mut self, trigger_id: &str) {
        if let Some(h) = self.handles.remove(trigger_id) {
            h.abort();
            tracing::info!(trigger_id, "worker stopped");
        }
    }

    /// Stop + spawn under one logical call.
    pub fn restart(&mut self, trigger: &Trigger, events: mpsc::Sender<TriggerEvent>) -> Result<()> {
        self.stop(&trigger.id);
        self.spawn(trigger, events)
    }

    #[doc(hidden)]
    pub fn worker_count(&self) -> usize {
        self.handles.len()
    }
}

fn parse_interval_ms(config_json: &str) -> Result<u64> {
    let v: serde_json::Value = serde_json::from_str(config_json)?;
    let n = v
        .get("interval_ms")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| anyhow::anyhow!("interval config missing `interval_ms: u64`"))?;
    Ok(n)
}
