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

use executor_evm::{DynProvider, EvmConfig};
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
    /// Phase 4 D-04: lazy provider clone (set by the MCP layer when the
    /// strategy is invoked). `None` for hosts that do not support
    /// `ctx.evm.*`; `Some` for runtime contexts built by the strategy_run
    /// handler.
    provider: Option<Arc<DynProvider>>,
    /// Phase 4 D-04: per-call timeout / RPC URL config.
    evm_config: EvmConfig,
    /// Phase 4 D-13: pending `journal_source_reads` rows for `ctx.evm.*`
    /// calls. Drained in [`RuntimeContext::flush`] alongside logs and the
    /// strategy_source marker.
    evm_reads: Vec<EvmReadRecord>,
    /// v1.2 Trigger Core: optional trigger event payload. `None` for
    /// manually-invoked strategies; `Some(payload)` for trigger-fired
    /// strategies. Surfaced as `ctx.event` (null when None).
    event: Option<serde_json::Value>,
    /// v1.7 (`ctx.price.usd`): shared price cache. Optional because tests
    /// build `RuntimeContext` without a real cache; production paths set
    /// this via [`RuntimeContext::with_price_cache`].
    price_cache: Option<Arc<executor_evm::PriceCache>>,
    /// v1.7 (`ctx.price.usd`): host's default chain_id (looked up via
    /// `eth_chainId` at strategy_run setup). `None` ⇒ JS callers that
    /// omit `chain_id` will see `null`.
    host_chain_id: Option<u64>,
}

/// One `ctx.evm.*` call's journal payload (Phase 4 D-13).
#[derive(Debug, Clone)]
pub struct EvmReadRecord {
    pub target: String,
    pub payload_json: serde_json::Value,
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
            provider: None,
            evm_config: EvmConfig::default(),
            evm_reads: Vec::new(),
            event: None,
            price_cache: None,
            host_chain_id: None,
        }
    }

    /// v1.7 (`ctx.price.usd`): attach the shared price cache + the host's
    /// default chain id. Builder-style; production callers (strategy_run)
    /// chain after `with_evm`.
    pub fn with_price_cache(
        mut self,
        cache: Arc<executor_evm::PriceCache>,
        chain_id: Option<u64>,
    ) -> Self {
        self.price_cache = Some(cache);
        self.host_chain_id = chain_id;
        self
    }

    /// v1.2 Trigger Core: attach the trigger event payload. Builder-style;
    /// mutates and returns the same context so the strategy_run handler can
    /// chain after `new`/`with_evm`.
    pub fn with_event(mut self, event: serde_json::Value) -> Self {
        self.event = Some(event);
        self
    }

    /// Phase 4: attach a lazy `Arc<DynProvider>` and the typed
    /// [`EvmConfig`]. Builder-style; mutates and returns the same context
    /// so the strategy_run handler can chain after `new`.
    pub fn with_evm(
        mut self,
        provider: Option<Arc<DynProvider>>,
        evm_config: EvmConfig,
    ) -> Self {
        self.provider = provider;
        self.evm_config = evm_config;
        self
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
        // 2. Phase 4 D-13: ctx.evm.* journal rows (kind="evm_read").
        //    MR-03 carry-forward: `?`-propagate serde failures via
        //    StateError::SerializationError — never silently fall back.
        for record in self.evm_reads.drain(..) {
            let payload_str = serde_json::to_string(&record.payload_json).map_err(|e| {
                StateError::SerializationError(format!(
                    "journal_source_reads.payload (evm_read): {e}"
                ))
            })?;
            store.record_source_read(
                &self.run_id,
                "evm_read",
                &record.target,
                Some(&payload_str),
            )?;
        }
        // 3. Logs (D-04 ctx.log → journal_logs).
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
    fn provider(&self) -> Option<&Arc<DynProvider>> {
        self.provider.as_ref()
    }
    fn evm_config(&self) -> &EvmConfig {
        &self.evm_config
    }
    fn record_evm_read(&mut self, target: String, payload: serde_json::Value) {
        self.evm_reads.push(EvmReadRecord {
            target,
            payload_json: payload,
        });
    }
    fn event(&self) -> Option<&serde_json::Value> {
        self.event.as_ref()
    }
    fn host_chain_id(&self) -> Option<u64> {
        self.host_chain_id
    }
    fn price_cache(&self) -> Option<&Arc<executor_evm::PriceCache>> {
        self.price_cache.as_ref()
    }
    /// Synchronous bridge to `executor_evm::resolve_usd_micros`. Production
    /// path uses `block_in_place` so the async resolver runs on the current
    /// tokio runtime without starving other tasks. Returns `None` when
    /// required state (provider / cache) is missing — matches the JS-side
    /// `null` semantic at the sandbox boundary.
    fn price_usd_micros(
        &self,
        chain_id: u64,
        token: executor_evm::Address,
        amount: executor_evm::U256,
    ) -> Option<u128> {
        let provider = self.provider.as_ref()?.clone();
        let cache = self.price_cache.as_ref()?.clone();
        let handle = tokio::runtime::Handle::try_current().ok()?;
        tokio::task::block_in_place(|| {
            handle.block_on(executor_evm::resolve_usd_micros(
                chain_id, token, amount, &provider, &cache,
            ))
        })
    }
}
