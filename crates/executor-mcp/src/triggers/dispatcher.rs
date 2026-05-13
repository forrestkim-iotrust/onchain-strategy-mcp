//! Dispatcher — single consumer of `mpsc::Receiver<TriggerEvent>`.
//!
//! For each accepted event:
//! 1. load trigger, drop if missing / disabled
//! 2. evaluate predicate (Stream B placeholder; see notes below)
//! 3. dedup check via state adapter
//! 4. fire strategy_run via [`crate::ExecutorServer::run_strategy_for_trigger`]
//! 5. record `trigger_events` row (`run_id` set on success, `skipped_reason`
//!    set otherwise).
//!
//! Predicate evaluation depends on Stream B's `Sandbox::evaluate_predicate`
//! and `RuntimeContext::with_event`. Those aren't merged yet; the dispatcher
//! degrades safely by treating predicates as "always true" and logging a
//! warn. Once Stream B lands, replace [`evaluate_predicate`] with the real
//! call.

use std::sync::Arc;

use executor_state::StateStore;
use tokio::sync::{Mutex, mpsc};

use super::event::TriggerEvent;
use super::state_adapter;
use crate::ExecutorServer;

pub struct Dispatcher {
    pub state: Arc<Mutex<StateStore>>,
    pub server: Arc<ExecutorServer>,
}

impl Dispatcher {
    pub fn new(state: Arc<Mutex<StateStore>>, server: Arc<ExecutorServer>) -> Self {
        Self { state, server }
    }

    pub async fn run(self, mut events: mpsc::Receiver<TriggerEvent>) {
        while let Some(e) = events.recv().await {
            if let Err(err) = self.handle_one(e).await {
                tracing::warn!(error = %err, "dispatcher: handle_one failed (continuing)");
            }
        }
        tracing::info!("dispatcher: event channel closed — exiting");
    }

    async fn handle_one(&self, e: TriggerEvent) -> anyhow::Result<()> {
        // 1. Load trigger.
        let state = self.state.clone();
        let tid_for_load = e.trigger_id.clone();
        let trigger_opt = tokio::task::spawn_blocking(move || {
            let store = state.blocking_lock();
            state_adapter::get_trigger(&store, &tid_for_load)
        })
        .await??;

        let Some(trigger) = trigger_opt else {
            tracing::warn!(trigger_id = %e.trigger_id, "dispatcher: trigger not found — dropping event");
            return Ok(());
        };
        if !trigger.enabled {
            // Don't even record — silent drop on disabled. Matches "stopped"
            // semantic; if the operator wants visibility they re-enable.
            return Ok(());
        }

        // 2. Predicate.
        if let Some(src) = trigger.predicate.as_deref() {
            match evaluate_predicate(src, &e.payload) {
                Ok(true) => { /* proceed */ }
                Ok(false) => {
                    self.record(&trigger.id, &e, None, Some("predicate_false"))
                        .await?;
                    return Ok(());
                }
                Err(err) => {
                    tracing::warn!(
                        trigger_id = %trigger.id,
                        error = %err,
                        "predicate eval failed — recording as predicate_error",
                    );
                    self.record(&trigger.id, &e, None, Some("predicate_error"))
                        .await?;
                    return Ok(());
                }
            }
        }

        // 3. Dedup.
        if let (Some(key), Some(window)) = (e.dedup_key.as_deref(), trigger.dedup_window_ms) {
            if window > 0 {
                let state = self.state.clone();
                let tid = trigger.id.clone();
                let key_owned = key.to_string();
                let dup = tokio::task::spawn_blocking(move || {
                    let store = state.blocking_lock();
                    state_adapter::check_dedup(&store, &tid, &key_owned, window)
                })
                .await??;
                if dup {
                    self.record(&trigger.id, &e, None, Some("dedup")).await?;
                    return Ok(());
                }
            }
        }

        // 4. Fire strategy_run.
        let run_id_result = self
            .server
            .run_strategy_for_trigger(trigger.strategy_id.clone(), Some(e.payload.clone()))
            .await;

        match run_id_result {
            Ok(run_id) => {
                self.record(&trigger.id, &e, Some(run_id.as_str()), None).await?;
            }
            Err(err) => {
                tracing::warn!(
                    trigger_id = %trigger.id,
                    strategy_id = %trigger.strategy_id,
                    error = %err,
                    "dispatcher: strategy_run failed — recording without run_id",
                );
                self.record(&trigger.id, &e, None, Some("strategy_error")).await?;
            }
        }
        Ok(())
    }

    async fn record(
        &self,
        trigger_id: &str,
        e: &TriggerEvent,
        run_id: Option<&str>,
        skipped_reason: Option<&str>,
    ) -> anyhow::Result<()> {
        let state = self.state.clone();
        let tid = trigger_id.to_string();
        let payload = e.payload.clone();
        let run_id = run_id.map(|s| s.to_string());
        let dedup_key = e.dedup_key.clone();
        let skipped_reason = skipped_reason.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || {
            let store = state.blocking_lock();
            state_adapter::record_trigger_event(
                &store,
                &tid,
                &payload,
                run_id.as_deref(),
                dedup_key.as_deref(),
                skipped_reason.as_deref(),
            )
        })
        .await??;
        Ok(())
    }
}

/// Placeholder predicate evaluator. Stream B replaces this with
/// `strategy_js::Sandbox::evaluate_predicate(source, event)`. Until that
/// lands the dispatcher treats every predicate as if it returned true, but
/// logs a warn so the gap is visible.
fn evaluate_predicate(_source: &str, _event: &serde_json::Value) -> anyhow::Result<bool> {
    tracing::warn!(
        "dispatcher: predicate evaluation is a no-op until Stream B (Sandbox::evaluate_predicate) merges — treating as true",
    );
    Ok(true)
}
