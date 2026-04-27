//! Append-only journal repository (D-06).
//!
//! Three tables, one row per event, ULID-keyed for stable ordering:
//! - `journal_source_reads` — STJ-03 (Phase 3 emits one row per run with
//!   `kind="strategy_source"`; Phase 4+ extends with EVM-read kinds).
//! - `journal_actions` — STJ-04 (one row per run carrying the outcome
//!   `noop` / `actions` / `validation_error` / `runtime_error`; Phase 5
//!   reserves `simulation_failure` / `policy_denied`).
//! - `journal_logs` — N rows per run, one per `ctx.log(...)` call.
//!
//! All inserts go through this module's free functions; the StateStore façade
//! methods are the public entry point. `phase3_emittable` is the gate
//! consulted before INSERTs into `journal_actions` (D-06 future-lock).
//!
//! ULIDs (D-05b carry-over) provide stable insertion-order sorting in
//! `list_*_for_run`. Same-second timestamps use `id ASC` as tie-breaker.

use crate::error::StateError;
use executor_core::schema::execution::JournalActionOutcome;
use rusqlite::{Connection, params};

#[derive(Debug, Clone)]
pub struct SourceReadEntry {
    pub id: String,
    pub run_id: String,
    pub kind: String,
    pub target: String,
    pub payload_json: Option<String>,
    pub recorded_at: String,
}

#[derive(Debug, Clone)]
pub struct ActionEntry {
    pub id: String,
    pub run_id: String,
    pub outcome: JournalActionOutcome,
    pub payload_json: String,
    pub recorded_at: String,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub id: String,
    pub run_id: String,
    pub message: String,
    pub recorded_at: String,
    /// Per-run monotonic counter assigned at INSERT (MR-04). Used as the
    /// primary tie-break for ORDER BY when same-second / same-millisecond
    /// inserts collide on `recorded_at`.
    pub seq: i64,
}

fn outcome_to_wire(o: JournalActionOutcome) -> &'static str {
    match o {
        JournalActionOutcome::Noop => "noop",
        JournalActionOutcome::Actions => "actions",
        JournalActionOutcome::ValidationError => "validation_error",
        JournalActionOutcome::RuntimeError => "runtime_error",
        JournalActionOutcome::SimulationFailure => "simulation_failure",
        JournalActionOutcome::PolicyDenied => "policy_denied",
    }
}

fn outcome_from_wire(s: &str) -> Result<JournalActionOutcome, StateError> {
    Ok(match s {
        "noop" => JournalActionOutcome::Noop,
        "actions" => JournalActionOutcome::Actions,
        "validation_error" => JournalActionOutcome::ValidationError,
        "runtime_error" => JournalActionOutcome::RuntimeError,
        "simulation_failure" => JournalActionOutcome::SimulationFailure,
        "policy_denied" => JournalActionOutcome::PolicyDenied,
        other => {
            return Err(StateError::Storage(format!(
                "unknown journal_actions.outcome in DB: {other}"
            )));
        }
    })
}

pub(crate) fn record_source_read(
    conn: &Connection,
    run_id: &str,
    kind: &str,
    target: &str,
    payload_json: Option<&str>,
) -> Result<String, StateError> {
    let id = ulid::Ulid::new().to_string();
    let now = super::strategies::now_rfc3339();
    conn.execute(
        "INSERT INTO journal_source_reads(id, run_id, kind, target, payload_json, recorded_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![&id, run_id, kind, target, payload_json, &now],
    )?;
    Ok(id)
}

pub(crate) fn record_action_outcome(
    conn: &Connection,
    run_id: &str,
    outcome: JournalActionOutcome,
    payload_json: &str,
) -> Result<String, StateError> {
    if !outcome.phase3_emittable() {
        return Err(StateError::InvalidInput(format!(
            "journal_actions.outcome {outcome:?} is reserved for Phase 5 and \
             cannot be emitted from Phase 3"
        )));
    }
    let id = ulid::Ulid::new().to_string();
    let now = super::strategies::now_rfc3339();
    conn.execute(
        "INSERT INTO journal_actions(id, run_id, outcome, payload_json, recorded_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![&id, run_id, outcome_to_wire(outcome), payload_json, &now],
    )?;
    Ok(id)
}

/// Compute the next per-run `seq` for `journal_logs` (MR-04). Phase 3 is
/// single-writer (one `Mutex<Connection>`), so the SELECT-then-INSERT pair
/// is race-free; the schema-level `UNIQUE (run_id, seq)` is a backstop.
fn next_log_seq(conn: &Connection, run_id: &str) -> Result<i64, StateError> {
    let next: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq), -1) + 1 FROM journal_logs WHERE run_id = ?1",
        params![run_id],
        |r| r.get(0),
    )?;
    Ok(next)
}

pub(crate) fn record_log(
    conn: &Connection,
    run_id: &str,
    message: &str,
) -> Result<String, StateError> {
    let id = ulid::Ulid::new().to_string();
    let now = super::strategies::now_rfc3339();
    let seq = next_log_seq(conn, run_id)?;
    conn.execute(
        "INSERT INTO journal_logs(id, run_id, message, recorded_at, seq) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![&id, run_id, message, &now, seq],
    )?;
    Ok(id)
}

/// Test-only helper: deterministic recorded_at for ordering tests
/// (mirrors `runs::insert_run_with_started_at`).
#[doc(hidden)]
pub(crate) fn record_log_with_time(
    conn: &Connection,
    run_id: &str,
    message: &str,
    recorded_at: &str,
) -> Result<String, StateError> {
    let id = ulid::Ulid::new().to_string();
    let seq = next_log_seq(conn, run_id)?;
    conn.execute(
        "INSERT INTO journal_logs(id, run_id, message, recorded_at, seq) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![&id, run_id, message, recorded_at, seq],
    )?;
    Ok(id)
}

pub(crate) fn list_source_reads_for_run(
    conn: &Connection,
    run_id: &str,
) -> Result<Vec<SourceReadEntry>, StateError> {
    let mut stmt = conn.prepare(
        "SELECT id, run_id, kind, target, payload_json, recorded_at \
         FROM journal_source_reads WHERE run_id = ?1 \
         ORDER BY recorded_at ASC, id ASC",
    )?;
    let rows = stmt
        .query_map(params![run_id], |r| {
            Ok(SourceReadEntry {
                id: r.get(0)?,
                run_id: r.get(1)?,
                kind: r.get(2)?,
                target: r.get(3)?,
                payload_json: r.get(4)?,
                recorded_at: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    Ok(rows)
}

pub(crate) fn list_actions_for_run(
    conn: &Connection,
    run_id: &str,
) -> Result<Vec<ActionEntry>, StateError> {
    let mut stmt = conn.prepare(
        "SELECT id, run_id, outcome, payload_json, recorded_at \
         FROM journal_actions WHERE run_id = ?1 \
         ORDER BY recorded_at ASC, id ASC",
    )?;
    let rows: Vec<(String, String, String, String, String)> = stmt
        .query_map(params![run_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    rows.into_iter()
        .map(|(id, rid, outcome_wire, payload_json, recorded_at)| {
            Ok(ActionEntry {
                id,
                run_id: rid,
                outcome: outcome_from_wire(&outcome_wire)?,
                payload_json,
                recorded_at,
            })
        })
        .collect()
}

pub(crate) fn list_logs_for_run(
    conn: &Connection,
    run_id: &str,
) -> Result<Vec<LogEntry>, StateError> {
    // MR-04: tie-break on `seq` (per-run monotonic at INSERT) — recorded_at
    // is RFC3339 second-granularity and ULID `id` is not insertion-ordered
    // within a same-millisecond bucket.
    let mut stmt = conn.prepare(
        "SELECT id, run_id, message, recorded_at, seq \
         FROM journal_logs WHERE run_id = ?1 \
         ORDER BY recorded_at ASC, seq ASC",
    )?;
    let rows = stmt
        .query_map(params![run_id], |r| {
            Ok(LogEntry {
                id: r.get(0)?,
                run_id: r.get(1)?,
                message: r.get(2)?,
                recorded_at: r.get(3)?,
                seq: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    Ok(rows)
}
