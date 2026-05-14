//! v1.4 strategy-bundle records-capture rows.
//!
//! `strategy_records_capture` holds one row per evaluated capture event —
//! produced by the executor-mcp records DSL evaluator at action-confirm time
//! and consumed by the `strategy://{id}/view` and `strategy://{id}/records`
//! resources.
//!
//! The schema lives in [`crate::schema`]; this module only owns the
//! INSERT / SELECT free-functions called from the [`crate::store::StateStore`]
//! façade.

use crate::error::StateError;
use rusqlite::{Connection, params};

/// One row of `strategy_records_capture` projected to the resource layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordCaptureEntry {
    pub id: i64,
    pub run_id: String,
    pub strategy_id: String,
    pub record_name: String,
    pub captured_at: String,
    /// Raw JSON-string payload (what the DSL evaluator produced for one match).
    pub payload_json: String,
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Insert one capture row. Capture is non-fatal: callers wrap this in their
/// own swallow-error guard so a failed INSERT never propagates into
/// action-confirm.
pub(crate) fn insert(
    conn: &Connection,
    run_id: &str,
    strategy_id: &str,
    record_name: &str,
    payload_json: &str,
) -> Result<(), StateError> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO strategy_records_capture \
           (run_id, strategy_id, record_name, captured_at, payload_json) \
           VALUES (?1, ?2, ?3, ?4, ?5)",
        params![run_id, strategy_id, record_name, &now, payload_json],
    )?;
    Ok(())
}

/// List capture rows for a strategy, newest-first, optionally filtered by
/// an exclusive `since` (RFC3339 string compare; caller validates shape).
/// `limit` is hard-capped to 500 to match the v1.4 design contract.
pub(crate) fn list_for_strategy(
    conn: &Connection,
    strategy_id: &str,
    since: Option<&str>,
    limit: u64,
) -> Result<Vec<RecordCaptureEntry>, StateError> {
    let capped = limit.min(500) as i64;
    let mut entries = Vec::new();
    match since {
        Some(s) => {
            let mut stmt = conn.prepare(
                "SELECT id, run_id, strategy_id, record_name, captured_at, payload_json \
                 FROM strategy_records_capture \
                 WHERE strategy_id = ?1 AND captured_at > ?2 \
                 ORDER BY captured_at DESC, id DESC \
                 LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![strategy_id, s, capped], |r| {
                Ok(RecordCaptureEntry {
                    id: r.get(0)?,
                    run_id: r.get(1)?,
                    strategy_id: r.get(2)?,
                    record_name: r.get(3)?,
                    captured_at: r.get(4)?,
                    payload_json: r.get(5)?,
                })
            })?;
            for row in rows {
                entries.push(row?);
            }
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT id, run_id, strategy_id, record_name, captured_at, payload_json \
                 FROM strategy_records_capture \
                 WHERE strategy_id = ?1 \
                 ORDER BY captured_at DESC, id DESC \
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![strategy_id, capped], |r| {
                Ok(RecordCaptureEntry {
                    id: r.get(0)?,
                    run_id: r.get(1)?,
                    strategy_id: r.get(2)?,
                    record_name: r.get(3)?,
                    captured_at: r.get(4)?,
                    payload_json: r.get(5)?,
                })
            })?;
            for row in rows {
                entries.push(row?);
            }
        }
    }
    Ok(entries)
}
