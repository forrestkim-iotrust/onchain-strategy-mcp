//! Local state CRUD shim — **temporary**. Stream A is adding
//! `executor_state::triggers::*` façade methods on `StateStore`. Until that
//! lands, this module talks to the raw SQLite connection (via the
//! `__test_conn` accessor, gated by feature for non-test callers? — we use
//! it directly for now; see migration note below).
//!
//! Migration note (merge of Streams A+D): replace each function here with a
//! call to the matching `StateStore::*_trigger*` method. The dispatcher and
//! pool only depend on the *signatures* defined here, so swapping the
//! implementation is a one-file change.

use anyhow::{Result, anyhow};
use executor_core::schema::trigger::{Trigger, TriggerKind};
use executor_state::StateStore;
use rusqlite::OptionalExtension;

/// Load a trigger row by id. Returns `Ok(None)` if missing.
pub fn get_trigger(store: &StateStore, id: &str) -> Result<Option<Trigger>> {
    let conn = store.__test_conn();
    let mut stmt = conn.prepare(
        "SELECT id, strategy_id, kind, config_json, predicate_js, enabled, \
         last_fired_at, created_at, dedup_window_ms \
         FROM triggers WHERE id = ?1",
    )?;
    let row = stmt
        .query_row([id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, Option<i64>>(8)?,
            ))
        })
        .optional()?;
    let Some((id, strategy_id, kind_s, config_json, predicate, enabled, last_fired_at, created_at, dedup_window_ms)) = row else {
        return Ok(None);
    };
    let kind = TriggerKind::from_db_str(&kind_s)
        .ok_or_else(|| anyhow!("unknown trigger kind {kind_s} for trigger {id}"))?;
    Ok(Some(Trigger {
        id,
        strategy_id,
        kind,
        config_json,
        predicate,
        enabled: enabled != 0,
        last_fired_at,
        created_at,
        dedup_window_ms: dedup_window_ms.map(|v| v as u64),
    }))
}

/// All enabled triggers (used at server boot to repopulate the worker pool).
pub fn list_enabled_triggers(store: &StateStore) -> Result<Vec<Trigger>> {
    let conn = store.__test_conn();
    let mut stmt = conn.prepare(
        "SELECT id, strategy_id, kind, config_json, predicate_js, enabled, \
         last_fired_at, created_at, dedup_window_ms \
         FROM triggers WHERE enabled = 1",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, Option<i64>>(8)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, strategy_id, kind_s, config_json, predicate, enabled, last_fired_at, created_at, dedup_window_ms) = row?;
        let kind = match TriggerKind::from_db_str(&kind_s) {
            Some(k) => k,
            None => {
                tracing::warn!(trigger_id = %id, kind = %kind_s, "skipping trigger with unknown kind at boot");
                continue;
            }
        };
        out.push(Trigger {
            id,
            strategy_id,
            kind,
            config_json,
            predicate,
            enabled: enabled != 0,
            last_fired_at,
            created_at,
            dedup_window_ms: dedup_window_ms.map(|v| v as u64),
        });
    }
    Ok(out)
}

/// Dedup check: is there a recent trigger_events row for `(trigger_id, key)`
/// fired within the last `window_ms` ms?
pub fn check_dedup(
    store: &StateStore,
    trigger_id: &str,
    dedup_key: &str,
    window_ms: u64,
) -> Result<bool> {
    if window_ms == 0 {
        return Ok(false);
    }
    let conn = store.__test_conn();
    let cutoff = chrono::Utc::now() - chrono::Duration::milliseconds(window_ms as i64);
    let cutoff_s = cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut stmt = conn.prepare(
        "SELECT 1 FROM trigger_events \
         WHERE trigger_id = ?1 AND dedup_key = ?2 AND fired_at >= ?3 \
         LIMIT 1",
    )?;
    let found: Option<i64> = stmt
        .query_row(rusqlite::params![trigger_id, dedup_key, cutoff_s], |r| r.get(0))
        .optional()?;
    Ok(found.is_some())
}

/// Insert a `trigger_events` row. `event_json` is the wire payload; `run_id`
/// is set when the event fired a strategy_run; `skipped_reason` carries the
/// dispatcher's decision when no run was created.
pub fn record_trigger_event(
    store: &StateStore,
    trigger_id: &str,
    event_json: &serde_json::Value,
    run_id: Option<&str>,
    dedup_key: Option<&str>,
    skipped_reason: Option<&str>,
) -> Result<String> {
    let conn = store.__test_conn();
    let id = ulid::Ulid::new().to_string();
    let fired_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let event_s = serde_json::to_string(event_json)?;
    conn.execute(
        "INSERT INTO trigger_events (id, trigger_id, event_json, fired_at, run_id, dedup_key, skipped_reason) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![id, trigger_id, event_s, fired_at, run_id, dedup_key, skipped_reason],
    )?;
    // Bump triggers.last_fired_at — only on actual fires, not skips, but the
    // simpler semantic of "last seen" is good enough for the spike.
    let _ = conn.execute(
        "UPDATE triggers SET last_fired_at = ?1 WHERE id = ?2",
        rusqlite::params![fired_at, trigger_id],
    );
    Ok(id)
}
