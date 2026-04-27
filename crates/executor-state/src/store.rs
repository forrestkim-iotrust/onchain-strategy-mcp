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

    pub fn update_run_status(
        &mut self,
        run_id: &str,
        status: RunStatus,
    ) -> Result<(), StateError> {
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
}
