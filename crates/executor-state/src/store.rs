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

use crate::{error::StateError, schema};
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

    /// **Test-only** raw connection accessor. Used by integration tests in
    /// `crates/executor-state/tests/` to assert SQL-level invariants
    /// (partial unique index, FK enforcement). Not part of the public API
    /// contract — do not depend on this from production code.
    #[doc(hidden)]
    pub fn __test_conn(&self) -> &Connection {
        &self.conn
    }
}
