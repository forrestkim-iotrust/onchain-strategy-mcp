//! Run base-model CRUD (D-04b, D-05a).
//!
//! Phase 2 lands the bare structs + module so `lib.rs` re-exports compile.
//! Full CRUD ships in Task 2 (this same plan).

// `RunStatus` lives in `executor-core::schema::execution` and is added in
// Task 2 of this plan. The `Run.status` field is wired up there too — Task 1
// only needs the module to exist so `lib.rs` re-exports compile.

#[derive(Debug, Clone)]
pub struct Run {
    pub id: String,
    pub strategy_id: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error: Option<String>,
}

/// Marker namespace for run-repo free functions. The actual CRUD entry points
/// live as free functions in this module + thin façade methods on `StateStore`.
#[derive(Debug, Clone, Copy)]
pub struct RunRepo;
