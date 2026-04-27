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

pub(crate) fn insert_run(
    conn: &Connection,
    strategy_id: &str,
    status: RunStatus,
) -> Result<String, StateError> {
    if !status.phase2_emittable() {
        return Err(StateError::InvalidInput(format!(
            "status {status:?} is reserved for Phase 5/6 and cannot be emitted from Phase 2"
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

pub(crate) fn update_run_status(
    conn: &Connection,
    run_id: &str,
    status: RunStatus,
) -> Result<(), StateError> {
    if !status.phase2_emittable() {
        return Err(StateError::InvalidInput(format!(
            "status {status:?} is reserved for Phase 5/6"
        )));
    }
    let finished_at = matches!(status, RunStatus::Succeeded | RunStatus::Failed)
        .then(super::strategies::now_rfc3339);
    let affected = conn.execute(
        "UPDATE runs SET status = ?1, finished_at = COALESCE(?2, finished_at) WHERE id = ?3",
        params![status_to_wire(status), finished_at, run_id],
    )?;
    if affected == 0 {
        return Err(StateError::NotFound(format!("run {run_id}")));
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
         FROM runs WHERE strategy_id = ?1 ORDER BY started_at DESC",
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
        .map(|(id, strategy_id, status_wire, started_at, finished_at, error)| {
            Ok(Run {
                id,
                strategy_id,
                status: status_from_wire(&status_wire)?,
                started_at,
                finished_at,
                error,
            })
        })
        .collect()
}
