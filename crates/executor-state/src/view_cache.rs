//! v1.12 Track B1 — last-known-good `strategy://{id}/view` cache.
//!
//! One row per `strategy_id`. `upsert` overwrites on every successful view
//! evaluation; `get` is consulted on failure to serve a stale envelope
//! (`confidence: "stale"`) instead of `data: null + confidence: "partial"`.
//! `delete` is invoked from `strategy_delete` so soft-deleted strategies
//! don't leave dangling cache rows.
//!
//! The schema lives in [`crate::schema`]; this module only owns the
//! INSERT / SELECT / DELETE free-functions called from the
//! [`crate::store::StateStore`] façade.

use crate::error::StateError;
use rusqlite::{Connection, OptionalExtension, params};

/// One row of `strategy_view_cache` projected to the resource layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewCacheRow {
    pub strategy_id: String,
    /// Full wrapped envelope body — `{ data, confidence: "full", logs, ... }` —
    /// as produced by the successful branch of `read_strategy_view`. On stale
    /// serve the caller overwrites `confidence` / `reason` / `remediation`
    /// and adds a `staleness` block; `data` is reused verbatim.
    pub body_json: String,
    /// RFC3339 millisecond timestamp of the last successful view write.
    pub succeeded_at: String,
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Insert or overwrite the cache row for `strategy_id`. Always re-stamps
/// `succeeded_at` to "now" — the row's age is the age of the *latest*
/// successful view, not the age of the first ever cached body.
pub(crate) fn upsert(
    conn: &Connection,
    strategy_id: &str,
    body_json: &str,
) -> Result<(), StateError> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO strategy_view_cache (strategy_id, body_json, succeeded_at) \
         VALUES (?1, ?2, ?3) \
         ON CONFLICT(strategy_id) DO UPDATE SET body_json = excluded.body_json, \
         succeeded_at = excluded.succeeded_at",
        params![strategy_id, body_json, &now],
    )?;
    Ok(())
}

/// Read the cached row for `strategy_id`, if any.
pub(crate) fn get(
    conn: &Connection,
    strategy_id: &str,
) -> Result<Option<ViewCacheRow>, StateError> {
    conn.query_row(
        "SELECT strategy_id, body_json, succeeded_at \
         FROM strategy_view_cache WHERE strategy_id = ?1",
        params![strategy_id],
        |r| {
            Ok(ViewCacheRow {
                strategy_id: r.get(0)?,
                body_json: r.get(1)?,
                succeeded_at: r.get(2)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// Delete the cache row for `strategy_id`. Returns `true` if a row was
/// removed, `false` if nothing was cached (idempotent — never errors on
/// missing). Called from the `strategy_delete` tool path.
pub(crate) fn delete(conn: &Connection, strategy_id: &str) -> Result<bool, StateError> {
    let n = conn.execute(
        "DELETE FROM strategy_view_cache WHERE strategy_id = ?1",
        params![strategy_id],
    )?;
    Ok(n > 0)
}
