//! `StateStore` — single `rusqlite::Connection` owner (D-03d, D-06).
//!
//! `StateStore` is **NOT** internally synchronised. Callers wrap this in
//! `Arc<tokio::sync::Mutex<StateStore>>` and enter a `spawn_blocking` +
//! `blocking_lock()` block before mutating calls (RESEARCH Pattern 2).
//! Holding the outer mutex across an `await` is forbidden (Pitfall 4).
//!
//! The bare `pub fn __test_conn` accessor is `#[doc(hidden)]` and only meant
//! for integration tests that need to exercise raw SQL invariants
//! (`partial_index_behaviour.rs`, `foreign_keys_enforced`). Production code
//! must go through the typed façade methods.

use crate::{error::StateError, runs, schema, strategies};
use executor_core::schema::execution::RunStatus;
use rusqlite::Connection;
use std::path::Path;

pub struct StateStore {
    pub(crate) conn: Connection,
}

impl StateStore {
    pub fn open(path: &Path) -> Result<Self, StateError> {
        let conn = schema::open_conn(path)?;
        Ok(Self { conn })
    }

    /// **Test-only** raw connection accessor.
    #[doc(hidden)]
    pub fn __test_conn(&self) -> &Connection {
        &self.conn
    }

    // ---- Strategy façade ----

    pub fn register_strategy(
        &mut self,
        name: &str,
        source: &str,
        description: Option<&str>,
        tags: Option<&[String]>,
    ) -> Result<strategies::RegisterOutcome, StateError> {
        strategies::register(&self.conn, name, source, description, tags)
    }

    pub fn list_strategies(
        &self,
        include_deleted: bool,
    ) -> Result<Vec<strategies::StrategySummary>, StateError> {
        strategies::list(&self.conn, include_deleted)
    }

    pub fn get_strategy_by_id(
        &self,
        id: &str,
    ) -> Result<Option<strategies::Strategy>, StateError> {
        strategies::get_by_id(&self.conn, id)
    }

    pub fn get_strategy_by_name(
        &self,
        name: &str,
    ) -> Result<Option<strategies::Strategy>, StateError> {
        strategies::get_by_name(&self.conn, name)
    }

    pub fn soft_delete_strategy(&mut self, id: &str) -> Result<String, StateError> {
        strategies::soft_delete(&self.conn, id)
    }

    pub fn is_strategy_deleted(&self, id: &str) -> Result<Option<bool>, StateError> {
        strategies::is_deleted(&self.conn, id)
    }

    // ---- Run façade ----

    pub fn insert_run(
        &mut self,
        strategy_id: &str,
        status: RunStatus,
    ) -> Result<String, StateError> {
        runs::insert_run(&self.conn, strategy_id, status)
    }

    /// **Test-only.** Insert a run with a caller-supplied `started_at` so
    /// integration tests can assert deterministic ordering in
    /// `list_runs_for_strategy` without `now_rfc3339`'s seconds granularity
    /// causing same-timestamp collisions. Production code paths MUST use
    /// [`StateStore::insert_run`].
    #[doc(hidden)]
    pub fn __test_insert_run_with_time(
        &mut self,
        strategy_id: &str,
        status: RunStatus,
        started_at: &str,
    ) -> Result<String, StateError> {
        runs::insert_run_with_started_at(&self.conn, strategy_id, status, started_at)
    }

    /// **Deprecated** — use [`StateStore::update_run_status_with_transition`]
    /// (D-12 transition guard). The unguarded API allows non-monotonic
    /// status mutations and is a defense-in-depth bypass surface (MR-02).
    /// Phase 5/6 simulation/policy-failure transitions MUST also route
    /// through the transition-guarded variant.
    #[deprecated(
        note = "use update_run_status_with_transition (D-12 transition guard); \
                the unguarded variant bypasses the state-machine and will be \
                removed once Phase 5/6 emit reserved-variant transitions \
                through the guarded API"
    )]
    pub fn update_run_status(
        &mut self,
        run_id: &str,
        status: RunStatus,
    ) -> Result<(), StateError> {
        #[allow(deprecated)]
        runs::update_run_status(&self.conn, run_id, status)
    }

    pub fn get_run(&self, run_id: &str) -> Result<Option<runs::Run>, StateError> {
        runs::get_run(&self.conn, run_id)
    }

    pub fn list_runs_for_strategy(
        &self,
        strategy_id: &str,
    ) -> Result<Vec<runs::Run>, StateError> {
        runs::list_runs_for_strategy(&self.conn, strategy_id)
    }

    /// D-12: transition-guarded status update. See
    /// `runs::update_run_status_with_transition` doc.
    pub fn update_run_status_with_transition(
        &mut self,
        run_id: &str,
        from: RunStatus,
        to: RunStatus,
    ) -> Result<(), StateError> {
        runs::update_run_status_with_transition(&self.conn, run_id, from, to)
    }

    // ---- Journal façade (Phase 3 D-06) ----

    pub fn record_source_read(
        &mut self,
        run_id: &str,
        kind: &str,
        target: &str,
        payload_json: Option<&str>,
    ) -> Result<String, StateError> {
        crate::journal::record_source_read(&self.conn, run_id, kind, target, payload_json)
    }

    pub fn record_action_outcome(
        &mut self,
        run_id: &str,
        outcome: executor_core::schema::execution::JournalActionOutcome,
        payload_json: &str,
    ) -> Result<String, StateError> {
        crate::journal::record_action_outcome(&self.conn, run_id, outcome, payload_json)
    }

    pub fn record_log(&mut self, run_id: &str, message: &str) -> Result<String, StateError> {
        crate::journal::record_log(&self.conn, run_id, message)
    }

    /// Test-only deterministic-time variant for ordering assertions.
    /// Mirrors `__test_insert_run_with_time`.
    #[doc(hidden)]
    pub fn __test_record_log_with_time(
        &mut self,
        run_id: &str,
        message: &str,
        recorded_at: &str,
    ) -> Result<String, StateError> {
        crate::journal::record_log_with_time(&self.conn, run_id, message, recorded_at)
    }

    pub fn list_source_reads_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<crate::journal::SourceReadEntry>, StateError> {
        crate::journal::list_source_reads_for_run(&self.conn, run_id)
    }

    pub fn list_actions_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<crate::journal::ActionEntry>, StateError> {
        crate::journal::list_actions_for_run(&self.conn, run_id)
    }

    pub fn list_logs_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<crate::journal::LogEntry>, StateError> {
        crate::journal::list_logs_for_run(&self.conn, run_id)
    }
}
