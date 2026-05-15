//! v1.12 Track B1: last-known-good cache for the strategy bundle `view`
//! function.
//!
//! When `view` evaluation fails (transient `evm revert`, RPC blip, …) the
//! dashboard falls back to the most recent successful body and renders a
//! STALE badge instead of a blank "you lost everything" state. The cache is
//! keyed by the active strategy row's `id` (NOT `lineage_id`) — a fresh
//! re-register under the same name mints a new `id` and therefore starts
//! the cache empty for the new version.
//!
//! Schema lives in [`crate::schema`]; this module owns the typed row + the
//! upsert/select/delete free functions called from the
//! [`crate::store::StateStore`] façade.

use crate::error::StateError;
use rusqlite::{Connection, OptionalExtension, params};

/// One row of `strategy_view_cache` projected to the resource layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewCacheRow {
    pub strategy_id: String,
    /// Full successful view-response body (caller-supplied canonical JSON).
    pub body_json: String,
    /// RFC3339 UTC timestamp stamped at upsert time.
    pub succeeded_at: String,
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Insert-or-update the cached body for `strategy_id`. Always stamps
/// `succeeded_at = now`; on conflict the existing row's body and timestamp
/// are both overwritten with `excluded.*`.
pub(crate) fn upsert(
    conn: &Connection,
    strategy_id: &str,
    body_json: &str,
) -> Result<(), StateError> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO strategy_view_cache (strategy_id, body_json, succeeded_at) \
           VALUES (?1, ?2, ?3) \
         ON CONFLICT(strategy_id) DO UPDATE SET \
           body_json = excluded.body_json, \
           succeeded_at = excluded.succeeded_at",
        params![strategy_id, body_json, &now],
    )?;
    Ok(())
}

/// Fetch the cached row for `strategy_id`. Returns `Ok(None)` when no
/// successful view has been recorded yet.
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
    .map_err(StateError::from)
}

/// Delete the cached row for `strategy_id`. No-op when the row is absent.
/// Intended for tests today; future purge / strategy-delete cleanup will
/// also call through here.
pub(crate) fn delete(conn: &Connection, strategy_id: &str) -> Result<(), StateError> {
    conn.execute(
        "DELETE FROM strategy_view_cache WHERE strategy_id = ?1",
        params![strategy_id],
    )?;
    Ok(())
}
