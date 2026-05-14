//! Run base-model CRUD (D-04b, D-05a).
//!
//! - `insert_run` rejects future-reserved statuses
//!   (`Canceled` / `SimulationDenied` / `PolicyDenied`) per D-05c — Phase 2
//!   code paths must never emit them.
//! - ULID identifiers (D-05b) — single-writer Phase 2 invariant means
//!   `Ulid::new()` suffices (no monotonic generator needed yet, Pitfall 6).
//! - `update_run_status` auto-fills `finished_at` on terminal statuses
//!   (`Succeeded` / `Failed`).
//! - FK violations surface as `StateError::Storage` (verified by
//!   `partial_index_behaviour::foreign_keys_enforced`).

use crate::error::StateError;
use executor_core::schema::execution::RunStatus;
use rusqlite::{Connection, OptionalExtension, params};

#[derive(Debug, Clone)]
pub struct Run {
    pub id: String,
    pub strategy_id: String,
    pub status: RunStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error: Option<String>,
}

/// Marker namespace — actual entry points are the free functions below
/// and the `StateStore` façade methods.
#[derive(Debug, Clone, Copy)]
pub struct RunRepo;

fn status_to_wire(s: RunStatus) -> &'static str {
    match s {
        RunStatus::Queued => "queued",
        RunStatus::Running => "running",
        RunStatus::Succeeded => "succeeded",
        RunStatus::Failed => "failed",
        RunStatus::Canceled => "canceled",
        RunStatus::SimulationDenied => "simulation_denied",
        RunStatus::PolicyDenied => "policy_denied",
    }
}

fn status_from_wire(s: &str) -> Result<RunStatus, StateError> {
    Ok(match s {
        "queued" => RunStatus::Queued,
        "running" => RunStatus::Running,
        "succeeded" => RunStatus::Succeeded,
        "failed" => RunStatus::Failed,
        "canceled" => RunStatus::Canceled,
        "simulation_denied" => RunStatus::SimulationDenied,
        "policy_denied" => RunStatus::PolicyDenied,
        other => {
            return Err(StateError::Storage(format!(
                "unknown run status in DB: {other}"
            )));
        }
    })
}

fn is_terminal_status(status: RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Succeeded
            | RunStatus::Failed
            | RunStatus::SimulationDenied
            | RunStatus::PolicyDenied
    )
}

pub(crate) fn insert_run(
    conn: &Connection,
    strategy_id: &str,
    status: RunStatus,
) -> Result<String, StateError> {
    if !status.phase5_emittable() {
        return Err(StateError::InvalidInput(format!(
            "status {status:?} is reserved for Phase 6 and cannot be emitted from Phase 2"
        )));
    }
    let id = ulid::Ulid::new().to_string();
    let started = super::strategies::now_rfc3339();
    conn.execute(
        "INSERT INTO runs(id, strategy_id, status, started_at) VALUES (?1, ?2, ?3, ?4)",
        params![&id, strategy_id, status_to_wire(status), &started],
    )?;
    Ok(id)
}

/// Test-only helper — insert a run with a caller-supplied `started_at`
/// timestamp so integration tests can assert deterministic
/// `list_runs_for_strategy` ordering without sleeping (Pitfall 6: same-second
/// `now_rfc3339` granularity collides under tight inserts). Production code
/// MUST use [`insert_run`].
#[doc(hidden)]
pub(crate) fn insert_run_with_started_at(
    conn: &Connection,
    strategy_id: &str,
    status: RunStatus,
    started_at: &str,
) -> Result<String, StateError> {
    if !status.phase5_emittable() {
        return Err(StateError::InvalidInput(format!(
            "status {status:?} is reserved for Phase 6 and cannot be emitted from Phase 2"
        )));
    }
    let id = ulid::Ulid::new().to_string();
    conn.execute(
        "INSERT INTO runs(id, strategy_id, status, started_at) VALUES (?1, ?2, ?3, ?4)",
        params![&id, strategy_id, status_to_wire(status), started_at],
    )?;
    Ok(id)
}

#[deprecated(
    note = "use update_run_status_with_transition (D-12 transition guard); \
            the unguarded variant bypasses the state-machine"
)]
pub(crate) fn update_run_status(
    conn: &Connection,
    run_id: &str,
    status: RunStatus,
) -> Result<(), StateError> {
    if !status.phase5_emittable() {
        return Err(StateError::InvalidInput(format!(
            "status {status:?} is reserved for Phase 6"
        )));
    }
    let finished_at = is_terminal_status(status).then(super::strategies::now_rfc3339);
    let affected = conn.execute(
        "UPDATE runs SET status = ?1, finished_at = COALESCE(?2, finished_at) WHERE id = ?3",
        params![status_to_wire(status), finished_at, run_id],
    )?;
    if affected == 0 {
        return Err(StateError::NotFound(format!("run {run_id}")));
    }
    Ok(())
}

/// D-12 transition guard. Atomically updates `runs.status` only when the
/// row's current status equals `from`. Returns `StateError::InvalidInput`
/// (NOT `NotFound`) when the row exists but is in a different state — the
/// caller's invariant is violated, not the row's existence.
///
/// `NotFound` is returned only when the row does not exist at all.
/// Reserved-variant gate (`phase5_emittable`) is enforced for both `from`
/// and `to` (you cannot transition INTO a reserved variant from Phase 3 code,
/// nor declare you expect a reserved one).
///
/// Closes 02-REVIEW MR-01: prior `update_run_status` was unconditional and
/// could overwrite a terminal status with an earlier-stage status. Phase 3's
/// `strategy_run` handler MUST use this transition-guarded API for every
/// status change.
///
/// `Succeeded → *` and `Failed → *` are Disallowed by D-12 (terminal). The
/// guard naturally rejects them via the `WHERE status = ?from` clause when
/// the caller asserts the wrong `from`. To prevent silent self-transitions
/// such as `Succeeded → Succeeded` (caller asserts `from = Succeeded`, row
/// IS Succeeded), the function additionally rejects any transition whose
/// `from` is a terminal status.
pub(crate) fn update_run_status_with_transition(
    conn: &Connection,
    run_id: &str,
    from: RunStatus,
    to: RunStatus,
) -> Result<(), StateError> {
    if !from.phase5_emittable() || !to.phase5_emittable() {
        return Err(StateError::InvalidInput(format!(
            "transition {from:?} → {to:?} involves a Phase 6 reserved status; \
             not allowed from Phase 3 code paths"
        )));
    }
    // D-12 + Phase 5: terminal statuses cannot transition to any other status,
    // even an idempotent self-transition (Succeeded → Succeeded).
    if is_terminal_status(from) {
        return Err(StateError::InvalidInput(format!(
            "run {run_id} is in terminal state {from:?}; transition to {to:?} is disallowed (D-12)"
        )));
    }
    let finished_at = is_terminal_status(to).then(super::strategies::now_rfc3339);
    let affected = conn.execute(
        "UPDATE runs SET status = ?1, finished_at = COALESCE(?2, finished_at) \
         WHERE id = ?3 AND status = ?4",
        params![
            status_to_wire(to),
            finished_at,
            run_id,
            status_to_wire(from)
        ],
    )?;
    if affected == 0 {
        // Distinguish NotFound vs InvalidInput by re-querying the row.
        let exists: bool = conn
            .query_row("SELECT 1 FROM runs WHERE id = ?1", params![run_id], |_| {
                Ok(())
            })
            .optional()?
            .is_some();
        if !exists {
            return Err(StateError::NotFound(format!("run {run_id}")));
        }
        return Err(StateError::InvalidInput(format!(
            "run {run_id} not in expected state {from:?} (transition guard)"
        )));
    }
    Ok(())
}

pub(crate) fn get_run(conn: &Connection, run_id: &str) -> Result<Option<Run>, StateError> {
    let row = conn
        .query_row(
            "SELECT id, strategy_id, status, started_at, finished_at, error \
             FROM runs WHERE id = ?1",
            params![run_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?;

    match row {
        None => Ok(None),
        Some((id, strategy_id, status_wire, started_at, finished_at, error)) => Ok(Some(Run {
            id,
            strategy_id,
            status: status_from_wire(&status_wire)?,
            started_at,
            finished_at,
            error,
        })),
    }
}

pub(crate) fn list_runs_for_strategy(
    conn: &Connection,
    strategy_id: &str,
) -> Result<Vec<Run>, StateError> {
    let mut stmt = conn.prepare(
        "SELECT id, strategy_id, status, started_at, finished_at, error \
         FROM runs WHERE strategy_id = ?1 ORDER BY started_at ASC, id ASC",
    )?;
    let rows = stmt
        .query_map(params![strategy_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;

    rows.into_iter()
        .map(
            |(id, strategy_id, status_wire, started_at, finished_at, error)| {
                Ok(Run {
                    id,
                    strategy_id,
                    status: status_from_wire(&status_wire)?,
                    started_at,
                    finished_at,
                    error,
                })
            },
        )
        .collect()
}

// ─────────── v1.4 Track C — execution://list backing query ───────────
//
// `list_runs` powers the `execution://list?strategy_id&since&status&limit`
// resource (see `executor-mcp::resources::read_execution_list`). Filters are
// composed dynamically — `strategy_id` is exact-match, `since` is an
// EXCLUSIVE lower bound on `started_at` (RFC3339 string compare), and
// `status` is the wire-name of a [`RunStatus`]. Sort order is newest-first
// (`started_at DESC, id DESC`); `id` is a ULID so the secondary key gives a
// deterministic tie-break for same-second inserts.
//
// `action_count` is `COUNT(*)` on `journal_actions` joined by `run_id` — one
// row per recorded outcome (`actions` / `noop` / `*_error` / `*_denied`),
// NOT one per executed action. The MCP layer is responsible for the
// semantic-name interpretation; here we only ship the raw count so callers
// don't have to round-trip a second query.

/// Hard cap on the number of summary rows `list_runs` will ever emit, even
/// when the caller passes a larger `limit`. Mirrors the v1.4 design contract
/// (`execution://list` resource).
pub const LIST_RUNS_LIMIT_CAP: u64 = 500;
/// Default `limit` applied when the caller does not specify one.
pub const LIST_RUNS_DEFAULT_LIMIT: u64 = 50;

/// Filter set for [`list_runs`]. All fields are optional; `None` means
/// "no constraint on this axis".
#[derive(Debug, Clone, Default)]
pub struct RunListFilter {
    /// Exact-match filter on `runs.strategy_id`.
    pub strategy_id: Option<String>,
    /// Exclusive lower bound on `runs.started_at` (RFC3339 string).
    /// Caller is responsible for validating the timestamp shape — `list_runs`
    /// does a raw string compare and SQLite's collation will happily compare
    /// any string.
    pub since: Option<String>,
    /// Exact-match filter on `runs.status`.
    pub status: Option<RunStatus>,
    /// Optional filter requiring at least one `journal_actions.outcome` row
    /// for the run to equal this wire-string (e.g. `"noop"`). The v1.4
    /// `execution://list?status=noop` filter routes through this field
    /// because `RunStatus` has no `Noop` variant — a no-op strategy has
    /// `RunStatus::Succeeded` plus a `journal_actions.outcome = 'noop'` row.
    pub journal_outcome: Option<String>,
    /// Max rows to return. `None` → [`LIST_RUNS_DEFAULT_LIMIT`].
    /// Values above [`LIST_RUNS_LIMIT_CAP`] are silently clamped down.
    pub limit: Option<u64>,
}

/// Summary row returned by [`list_runs`]. Lighter than [`Run`] so the
/// resource layer doesn't pay a `serde::Value` cost per row; `action_count`
/// is a `COUNT(*)` on `journal_actions` keyed by `run_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    pub run_id: String,
    pub strategy_id: String,
    pub status: RunStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub action_count: i64,
}

pub(crate) fn list_runs(
    conn: &Connection,
    filter: &RunListFilter,
) -> Result<Vec<RunSummary>, StateError> {
    // Compose WHERE + params dynamically. Each clause is gated on its
    // `Option::is_some()` so an empty filter matches all rows.
    let mut where_clauses: Vec<&'static str> = Vec::new();
    let mut bound: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(sid) = &filter.strategy_id {
        where_clauses.push("r.strategy_id = ?");
        bound.push(rusqlite::types::Value::Text(sid.clone()));
    }
    if let Some(since) = &filter.since {
        where_clauses.push("r.started_at > ?");
        bound.push(rusqlite::types::Value::Text(since.clone()));
    }
    if let Some(status) = filter.status {
        where_clauses.push("r.status = ?");
        bound.push(rusqlite::types::Value::Text(status_to_wire(status).to_string()));
    }
    if let Some(outcome) = &filter.journal_outcome {
        // EXISTS subquery is correlated to the run id from the outer query.
        // This is faster than a join + DISTINCT for journal_outcome=noop on
        // a workload where most runs have a small fixed number of action rows.
        where_clauses
            .push("EXISTS (SELECT 1 FROM journal_actions ja2 WHERE ja2.run_id = r.id AND ja2.outcome = ?)");
        bound.push(rusqlite::types::Value::Text(outcome.clone()));
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    let limit = filter
        .limit
        .unwrap_or(LIST_RUNS_DEFAULT_LIMIT)
        .min(LIST_RUNS_LIMIT_CAP);
    bound.push(rusqlite::types::Value::Integer(limit as i64));

    // LEFT JOIN with COUNT(*) on journal_actions so runs with zero journaled
    // actions still appear (count = 0). GROUP BY pinned to runs.id keeps the
    // aggregate scoped per row. ORDER BY (started_at DESC, id DESC) — newest
    // first; id is a ULID so it doubles as a stable same-second tie-break.
    let sql = format!(
        "SELECT r.id, r.strategy_id, r.status, r.started_at, r.finished_at, \
                COUNT(ja.id) AS action_count \
         FROM runs r \
         LEFT JOIN journal_actions ja ON ja.run_id = r.id \
         {where_sql} \
         GROUP BY r.id \
         ORDER BY r.started_at DESC, r.id DESC \
         LIMIT ?"
    );

    let mut stmt = conn.prepare(&sql)?;
    let params_iter: Vec<&dyn rusqlite::ToSql> =
        bound.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
    let rows = stmt
        .query_map(params_iter.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;

    rows.into_iter()
        .map(
            |(run_id, strategy_id, status_wire, started_at, finished_at, action_count)| {
                Ok(RunSummary {
                    run_id,
                    strategy_id,
                    status: status_from_wire(&status_wire)?,
                    started_at,
                    finished_at,
                    action_count,
                })
            },
        )
        .collect()
}
