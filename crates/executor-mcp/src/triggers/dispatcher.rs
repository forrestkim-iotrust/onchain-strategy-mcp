//! `Dispatcher` — drains `TriggerEvent`s off the channel, applies predicate
//! + dedup gates, and invokes `ExecutorServer::run_strategy_with_event`.

use std::sync::{Arc, Weak};

use executor_state::StateStore;
use strategy_js::Sandbox;
use tokio::sync::Mutex;

use crate::ExecutorServer;
use crate::triggers::event::TriggerEvent;

pub struct Dispatcher {
    pub state: Arc<Mutex<StateStore>>,
    pub server: Weak<ExecutorServer>,
}

fn payload_json(payload: &serde_json::Value) -> Option<String> {
    // Skip serializing Null — the store treats `None` as "no payload recorded".
    if payload.is_null() {
        None
    } else {
        serde_json::to_string(payload).ok()
    }
}

impl Dispatcher {
    pub async fn run(self, mut events: tokio::sync::mpsc::Receiver<TriggerEvent>) {
        while let Some(e) = events.recv().await {
            let event_json = payload_json(&e.payload);
            // 1. Load trigger.
            let trigger = {
                let store = self.state.lock().await;
                match store.get_trigger(&e.trigger_id) {
                    Ok(Some(t)) => t,
                    Ok(None) => {
                        tracing::debug!(trigger_id = %e.trigger_id, "trigger not found; dropping event");
                        continue;
                    }
                    Err(err) => {
                        tracing::warn!(trigger_id = %e.trigger_id, error = %err, "load trigger failed");
                        continue;
                    }
                }
            };
            if !trigger.enabled {
                continue;
            }

            // 2. Predicate gate.
            if let Some(src) = &trigger.predicate {
                match Sandbox::evaluate_predicate(src, &e.payload) {
                    Ok(true) => {}
                    Ok(false) => {
                        let _ = {
                            let mut store = self.state.lock().await;
                            store.record_trigger_event(
                                &trigger.id,
                                event_json.as_deref(),
                                None,
                                e.dedup_key.as_deref(),
                                Some("predicate_false"),
                            )
                        };
                        continue;
                    }
                    Err(err) => {
                        tracing::warn!(
                            trigger_id = %trigger.id,
                            error = %err,
                            "predicate evaluation error; skipping event",
                        );
                        let _ = {
                            let mut store = self.state.lock().await;
                            store.record_trigger_event(
                                &trigger.id,
                                event_json.as_deref(),
                                None,
                                e.dedup_key.as_deref(),
                                Some("predicate_error"),
                            )
                        };
                        continue;
                    }
                }
            }

            // 3. Dedup gate.
            if let (Some(key), Some(window)) = (
                e.dedup_key.as_ref(),
                trigger.dedup_window_ms.filter(|w| *w > 0),
            ) {
                let deduped = {
                    let store = self.state.lock().await;
                    store.check_trigger_dedup(&trigger.id, key, window)
                };
                match deduped {
                    Ok(true) => {
                        let _ = {
                            let mut store = self.state.lock().await;
                            store.record_trigger_event(
                                &trigger.id,
                                event_json.as_deref(),
                                None,
                                Some(key),
                                Some("dedup"),
                            )
                        };
                        continue;
                    }
                    Ok(false) => {}
                    Err(err) => {
                        tracing::warn!(
                            trigger_id = %trigger.id,
                            error = %err,
                            "dedup check failed; proceeding without dedup",
                        );
                    }
                }
            }

            // 4. Run the strategy.
            let server = match self.server.upgrade() {
                Some(s) => s,
                None => {
                    tracing::info!("dispatcher exiting: server dropped");
                    break;
                }
            };
            let result = server
                .run_strategy_with_event(&trigger.strategy_id, Some(e.payload.clone()))
                .await;
            match result {
                Ok((run_id, _outcome)) => {
                    let mut store = self.state.lock().await;
                    if let Err(err) = store.record_trigger_event(
                        &trigger.id,
                        event_json.as_deref(),
                        Some(&run_id),
                        e.dedup_key.as_deref(),
                        None,
                    ) {
                        tracing::warn!(trigger_id = %trigger.id, error = %err, "record_trigger_event (success) failed");
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        trigger_id = %trigger.id,
                        strategy_id = %trigger.strategy_id,
                        error = ?err,
                        "strategy run failed",
                    );
                    let mut store = self.state.lock().await;
                    if let Err(err2) = store.record_trigger_event(
                        &trigger.id,
                        event_json.as_deref(),
                        None,
                        e.dedup_key.as_deref(),
                        Some("run_error"),
                    ) {
                        tracing::warn!(trigger_id = %trigger.id, error = %err2, "record_trigger_event (run_error) failed");
                    }
                }
            }
        }
    }
}
