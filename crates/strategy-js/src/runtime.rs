//! `RuntimeContext` — production [`CtxHost`] impl backed by an
//! `Arc<tokio::sync::Mutex<StateStore>>`.
//!
//! Buffers logs during JS execution; `flush` drains them to `journal_logs`
//! and emits the Phase-3 source-read marker (STJ-03) in a single mutex
//! acquisition.
//!
//! Plan 03-03's `strategy_run` handler is the single caller. It runs
//! `Sandbox::execute(source, &mut runtime_ctx)` inside a
//! `tokio::task::spawn_blocking` block (rquickjs `Runtime` is `!Sync`) and
//! then invokes `runtime_ctx.flush()` from a context that may also be inside
//! `spawn_blocking` (we use `state.blocking_lock()`, which only works outside
//! the async runtime — match the Phase-2 `executor-mcp` invariant).
//!
//! This module does NOT touch `runs.status` — D-12 transitions are the
//! handler's responsibility. RuntimeContext only owns ctx surface fields +
//! source-read marker + log flush.

use std::sync::Arc;
use tokio::sync::Mutex;

use executor_state::{StateError, StateStore};

use crate::sandbox::CtxHost;

/// Snapshot-of-now provider — Phase 3 default uses `chrono::Utc::now`,
/// tests inject a fixed value for determinism. Boxed (`Arc<dyn Fn>`) to
/// avoid leaking the generic into `RuntimeContext`'s public type.
pub type NowMillisProvider = Arc<dyn Fn() -> i64 + Send + Sync>;

pub struct RuntimeContext {
    state: Arc<Mutex<StateStore>>,
    strategy_id: String,
    strategy_name: String,
    run_id: String,
    now_provider: NowMillisProvider,
    log_buffer: Vec<String>,
    /// True until [`RuntimeContext::flush`] writes the per-run source-read
    /// marker. Cleared after a successful flush so a second `flush()` call
    /// is a no-op (idempotent).
    source_read_pending: bool,
}

impl RuntimeContext {
    pub fn new(
        state: Arc<Mutex<StateStore>>,
        strategy_id: String,
        strategy_name: String,
        run_id: String,
        now_provider: NowMillisProvider,
    ) -> Self {
        Self {
            state,
            strategy_id,
            strategy_name,
            run_id,
            now_provider,
            log_buffer: Vec::new(),
            source_read_pending: true,
        }
    }

    /// Default chrono-backed clock provider (Phase-3 v1 — D-04).
    pub fn default_clock() -> NowMillisProvider {
        Arc::new(|| chrono::Utc::now().timestamp_millis())
    }

    /// Drain the buffered logs and the source-read marker into the
    /// `StateStore`. Called from the strategy_run handler AFTER
    /// `Sandbox::execute` returns control. Caller MUST run this from a
    /// blocking context (outside an async runtime, or inside
    /// `tokio::task::spawn_blocking`) because `state.blocking_lock()` is
    /// invoked here.
    ///
    /// Idempotent: a second call after a successful flush writes zero new
    /// rows (`source_read_pending` is cleared, `log_buffer` is drained).
    pub fn flush(&mut self) -> Result<(), StateError> {
        let mut store = self.state.blocking_lock();
        // 1. Source-read marker (STJ-03).
        if self.source_read_pending {
            store.record_source_read(
                &self.run_id,
                "strategy_source",
                &self.strategy_id,
                None,
            )?;
            self.source_read_pending = false;
        }
        // 2. Logs (D-04 ctx.log → journal_logs).
        for msg in self.log_buffer.drain(..) {
            store.record_log(&self.run_id, &msg)?;
        }
        Ok(())
    }
}

impl CtxHost for RuntimeContext {
    fn strategy_id(&self) -> &str {
        &self.strategy_id
    }
    fn strategy_name(&self) -> &str {
        &self.strategy_name
    }
    fn run_id(&self) -> &str {
        &self.run_id
    }
    fn now_millis(&self) -> i64 {
        (self.now_provider)()
    }
    fn append_log(&mut self, message: String) {
        self.log_buffer.push(message);
    }
}
