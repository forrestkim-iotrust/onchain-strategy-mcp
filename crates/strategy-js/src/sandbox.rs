//! Sandbox surface — Task 2 implements `execute`.
//!
//! Public API is locked here so dependent crates can compile against the
//! signature even before Task 2 lands the body.

use crate::error::RuntimeError;

/// Host-side context the strategy sees as `ctx`. Phase 3 buffers `ctx.log`
/// calls in `append_log` (no DB IO inside JS execution — RESEARCH Pitfall 2);
/// Plan 03-02 swaps this trait's impl from [`CtxStub`] to `RuntimeContext`
/// which flushes the buffer to `journal_logs` after `execute` returns.
pub trait CtxHost {
    fn strategy_id(&self) -> &str;
    fn strategy_name(&self) -> &str;
    fn run_id(&self) -> &str;
    fn now_millis(&self) -> i64;
    fn append_log(&mut self, message: String);
}

/// In-memory `CtxHost` implementation used by Phase-3 unit tests and as the
/// type Plan 03-02 will replace at the MCP boundary.
#[derive(Debug, Default)]
pub struct CtxStub {
    pub strategy_id: String,
    pub strategy_name: String,
    pub run_id: String,
    pub logs: Vec<String>,
}

impl CtxHost for CtxStub {
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
        // Stub clock — Plan 03-02's RuntimeContext uses chrono::Utc::now.
        // Tests can pre-populate this if determinism is needed.
        0
    }
    fn append_log(&mut self, message: String) {
        self.logs.push(message);
    }
}

/// Synchronous JavaScript sandbox. Construction is free (unit struct);
/// `execute` constructs a fresh rquickjs `Runtime + Context::base` per call.
pub struct Sandbox;

impl Sandbox {
    /// Evaluate a strategy under the D-03 budgets and the D-04 `ctx`
    /// surface. **Caller wraps in `tokio::task::spawn_blocking`** —
    /// rquickjs `Runtime` is `!Sync` without the `parallel` feature.
    ///
    /// Returns the strategy's return value as a `serde_json::Value`,
    /// which Plan 03-03 will validate against
    /// `executor-core::schema::action::Action`.
    ///
    /// **Task 2 implements the body.**
    #[allow(clippy::unimplemented)]
    pub fn execute<H: CtxHost>(
        _source: &str,
        _host: &mut H,
    ) -> Result<serde_json::Value, RuntimeError> {
        unimplemented!("Task 2 lands the rquickjs evaluation body")
    }
}
