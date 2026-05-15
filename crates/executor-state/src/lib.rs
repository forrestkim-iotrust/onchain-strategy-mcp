#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
//! `executor-state` — local SQLite persistence for strategies and runs.
//!
//! - `error`: typed [`StateError`] (D-06a) — mapped to MCP error codes in Plan 02-02.
//! - `schema`: `open_conn` runs pragmas (WAL / synchronous=NORMAL / foreign_keys=ON, D-03c)
//!   and idempotent `CREATE TABLE IF NOT EXISTS` DDL (D-03b, D-04).
//! - `store`: [`StateStore`] owns a single `rusqlite::Connection` (D-03d). Async bridging
//!   (`Arc<tokio::sync::Mutex<StateStore>>` + `spawn_blocking`) lives in `executor-mcp`.
//! - `strategies`: content-addressed CRUD (D-01..D-02, D-07a..c).
//! - `runs`: base CRUD (D-04b, D-05a) — Phase 3+ extends.

pub mod error;
pub mod executions;
pub mod journal;
pub mod policy_revisions;
pub mod records_capture;
pub mod runs;
pub mod schema;
pub mod store;
pub mod strategies;
pub mod triggers;
pub mod view_cache;

pub use error::StateError;
pub use executions::ExecutionActionEntry;
pub use journal::{
    ActionEntry, DecisionEntry, DecisionGate, DecisionVerdict, LogEntry, SourceReadEntry,
};
pub use policy_revisions::{PolicyRevision, PolicyRevisionSummary};
pub use records_capture::RecordCaptureEntry;
pub use runs::{
    LIST_RUNS_DEFAULT_LIMIT, LIST_RUNS_LIMIT_CAP, Run, RunListFilter, RunRepo, RunSummary,
};
pub use store::StateStore;
pub use strategies::{
    RegisterOutcome, Strategy, StrategySummary, hash_bundle, hash_bundle_with_lineage,
    hash_source,
};
pub use triggers::TriggerRegisterOutcome;
pub use view_cache::ViewCacheRow;
