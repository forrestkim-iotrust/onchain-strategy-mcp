//! Execution action repository for local managed execution (Phase 6).

use crate::error::StateError;
use rusqlite::{Connection, OptionalExtension, params};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionActionEntry {
    pub id: String,
    pub run_id: String,
    pub action_index: i64,
    pub signer_address: String,
    pub tx_hash: Option<String>,
    pub status: String,
    pub receipt_status: Option<String>,
    pub gas_used: Option<String>,
    pub error_kind: Option<String>,
    pub error_detail: Option<String>,
    pub recorded_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NewExecutionBroadcast<'a> {
    pub run_id: &'a str,
    pub action_index: i64,
    pub signer_address: &'a str,
    pub tx_hash: &'a str,
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub(crate) fn record_broadcast(
    conn: &Connection,
    broadcast: NewExecutionBroadcast<'_>,
) -> Result<String, StateError> {
    let now = now_rfc3339();
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM execution_actions WHERE run_id = ?1 AND action_index = ?2",
            params![broadcast.run_id, broadcast.action_index],
            |r| r.get(0),
        )
        .optional()?;

    match existing_id {
        Some(id) => {
            conn.execute(
                "UPDATE execution_actions SET signer_address = ?1, tx_hash = ?2, status = 'broadcasted', \
                 receipt_status = NULL, gas_used = NULL, error_kind = NULL, error_detail = NULL, updated_at = ?3 \
                 WHERE id = ?4",
                params![broadcast.signer_address, broadcast.tx_hash, &now, &id],
            )?;
            Ok(id)
        }
        None => {
            let id = ulid::Ulid::new().to_string();
            conn.execute(
                "INSERT INTO execution_actions \
                 (id, run_id, action_index, signer_address, tx_hash, status, recorded_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, 'broadcasted', ?6, ?7)",
                params![
                    &id,
                    broadcast.run_id,
                    broadcast.action_index,
                    broadcast.signer_address,
                    broadcast.tx_hash,
                    &now,
                    &now,
                ],
            )?;
            Ok(id)
        }
    }
}

pub(crate) fn record_receipt_success(
    conn: &Connection,
    run_id: &str,
    action_index: i64,
    receipt_status: &str,
    gas_used: &str,
) -> Result<(), StateError> {
    let status = match receipt_status {
        "success" => "confirmed",
        "reverted" => "failed",
        other => {
            return Err(StateError::InvalidInput(format!(
                "unknown execution receipt status: {other}"
            )));
        }
    };
    let now = now_rfc3339();
    let affected = conn.execute(
        "UPDATE execution_actions SET status = ?1, receipt_status = ?2, gas_used = ?3, \
         error_kind = NULL, error_detail = NULL, updated_at = ?4 \
         WHERE run_id = ?5 AND action_index = ?6",
        params![status, receipt_status, gas_used, &now, run_id, action_index],
    )?;
    if affected == 0 {
        return Err(StateError::NotFound(format!(
            "execution action {run_id}/{action_index}"
        )));
    }
    Ok(())
}

pub(crate) fn record_execution_error(
    conn: &Connection,
    run_id: &str,
    action_index: i64,
    error_kind: &str,
    error_detail: Option<&str>,
) -> Result<(), StateError> {
    let now = now_rfc3339();
    let affected = conn.execute(
        "UPDATE execution_actions SET status = 'failed', error_kind = ?1, error_detail = ?2, updated_at = ?3 \
         WHERE run_id = ?4 AND action_index = ?5",
        params![error_kind, error_detail, &now, run_id, action_index],
    )?;
    if affected == 0 {
        let id = ulid::Ulid::new().to_string();
        conn.execute(
            "INSERT INTO execution_actions \
             (id, run_id, action_index, signer_address, status, error_kind, error_detail, recorded_at, updated_at) \
             VALUES (?1, ?2, ?3, '', 'failed', ?4, ?5, ?6, ?7)",
            params![&id, run_id, action_index, error_kind, error_detail, &now, &now],
        )?;
    }
    Ok(())
}

pub(crate) fn list_executions_for_run(
    conn: &Connection,
    run_id: &str,
) -> Result<Vec<ExecutionActionEntry>, StateError> {
    let mut stmt = conn.prepare(
        "SELECT id, run_id, action_index, signer_address, tx_hash, status, receipt_status, gas_used, \
         error_kind, error_detail, recorded_at, updated_at \
         FROM execution_actions WHERE run_id = ?1 ORDER BY action_index ASC",
    )?;
    let rows = stmt
        .query_map(params![run_id], |r| {
            Ok(ExecutionActionEntry {
                id: r.get(0)?,
                run_id: r.get(1)?,
                action_index: r.get(2)?,
                signer_address: r.get(3)?,
                tx_hash: r.get(4)?,
                status: r.get(5)?,
                receipt_status: r.get(6)?,
                gas_used: r.get(7)?,
                error_kind: r.get(8)?,
                error_detail: r.get(9)?,
                recorded_at: r.get(10)?,
                updated_at: r.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    Ok(rows)
}
