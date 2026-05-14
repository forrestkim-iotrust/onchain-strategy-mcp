//! Trigger CRUD (v1.2 Trigger Core — Stream A).
//!
//! Mirrors `strategies.rs` patterns: content-addressed id (sha256 over the
//! tuple `(strategy_id, kind, config_json, predicate_js)`), idempotent
//! register, parameterised SQL, no raw row leaking past the typed façade.
//!
//! Stream C lands this file with the MCP tool surface so the build is
//! self-consistent ahead of the streams merge. The contract here matches the
//! design lock at `.planning/v1.2-TRIGGER-CORE-DESIGN.md`.

use crate::error::StateError;
use crate::strategies::now_rfc3339;
use executor_core::schema::trigger::{
    RegisterTriggerInput, Trigger, TriggerEvent, TriggerKind, TriggerListFilter, TriggerSummary,
};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub enum TriggerRegisterOutcome {
    Created(Trigger),
    AlreadyExists(Trigger),
}

fn canonical_config(config: &serde_json::Value) -> String {
    // Stable serialization — serde_json preserves object key order from the
    // input map. For idempotency we re-encode through a canonical roundtrip
    // (sorted keys) so semantically-equal configs hash equal.
    fn canonicalize(v: &serde_json::Value) -> serde_json::Value {
        match v {
            serde_json::Value::Object(map) => {
                let mut entries: Vec<_> = map.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));
                let mut out = serde_json::Map::new();
                for (k, vv) in entries {
                    out.insert(k.clone(), canonicalize(vv));
                }
                serde_json::Value::Object(out)
            }
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(canonicalize).collect())
            }
            other => other.clone(),
        }
    }
    serde_json::to_string(&canonicalize(config)).unwrap_or_else(|_| "{}".to_string())
}

fn hash_trigger(
    strategy_id: &str,
    kind: TriggerKind,
    config_json: &str,
    predicate_js: Option<&str>,
) -> String {
    let mut h = Sha256::new();
    h.update(strategy_id.as_bytes());
    h.update(b"\x1f");
    h.update(kind.as_wire().as_bytes());
    h.update(b"\x1f");
    h.update(config_json.as_bytes());
    h.update(b"\x1f");
    if let Some(p) = predicate_js {
        h.update(p.as_bytes());
    }
    hex::encode(h.finalize())
}

fn parse_kind(s: &str) -> Result<TriggerKind, StateError> {
    TriggerKind::from_wire(s)
        .ok_or_else(|| StateError::Storage(format!("unknown trigger kind in DB: {s}")))
}

#[allow(clippy::too_many_arguments)]
fn map_trigger_row(
    id: String,
    strategy_id: String,
    kind: String,
    config_json: String,
    predicate_js: Option<String>,
    enabled: i64,
    last_fired_at: Option<String>,
    created_at: String,
    dedup_window_ms: Option<i64>,
    note: Option<String>,
    strategy_lineage_id: Option<String>,
) -> Result<Trigger, StateError> {
    Ok(Trigger {
        id,
        strategy_id,
        kind: parse_kind(&kind)?,
        config_json,
        predicate: predicate_js,
        enabled: enabled != 0,
        last_fired_at,
        created_at,
        dedup_window_ms: dedup_window_ms.and_then(|n| u64::try_from(n).ok()),
        note,
        strategy_lineage_id,
    })
}

pub(crate) fn register(
    conn: &Connection,
    input: RegisterTriggerInput,
) -> Result<TriggerRegisterOutcome, StateError> {
    // Ensure strategy exists AND grab its lineage_id so the trigger row can
    // attach to a LINEAGE (survives view/records-spec re-registrations of
    // the same name) rather than a specific version.
    let lineage_row: Option<Option<String>> = conn
        .query_row(
            "SELECT lineage_id FROM strategies WHERE id = ?1",
            params![&input.strategy_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?;
    let strategy_lineage_id = match lineage_row {
        None => {
            return Err(StateError::NotFound(format!(
                "strategy {}",
                input.strategy_id
            )));
        }
        Some(opt) => opt,
    };

    let config_json = canonical_config(&input.config);
    let id = hash_trigger(
        &input.strategy_id,
        input.kind,
        &config_json,
        input.predicate.as_deref(),
    );

    if let Some(existing) = get_by_id(conn, &id)? {
        return Ok(TriggerRegisterOutcome::AlreadyExists(existing));
    }

    let now = now_rfc3339();
    let dedup = input.dedup_window_ms.map(|n| n as i64);
    conn.execute(
        "INSERT INTO triggers(id, strategy_id, kind, config_json, predicate_js,
                              enabled, last_fired_at, created_at, dedup_window_ms,
                              note, strategy_lineage_id)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, NULL, ?6, ?7, ?8, ?9)",
        params![
            &id,
            &input.strategy_id,
            input.kind.as_wire(),
            &config_json,
            input.predicate.as_deref(),
            &now,
            dedup,
            input.note.as_deref(),
            &strategy_lineage_id,
        ],
    )?;

    Ok(TriggerRegisterOutcome::Created(Trigger {
        id,
        strategy_id: input.strategy_id,
        kind: input.kind,
        config_json,
        predicate: input.predicate,
        enabled: true,
        last_fired_at: None,
        created_at: now,
        dedup_window_ms: input.dedup_window_ms,
        note: input.note,
        strategy_lineage_id,
    }))
}

pub(crate) fn list(
    conn: &Connection,
    filter: Option<&TriggerListFilter>,
) -> Result<Vec<TriggerSummary>, StateError> {
    let mut sql = String::from(
        "SELECT id, strategy_id, kind, enabled, last_fired_at, created_at, \
                note, strategy_lineage_id \
         FROM triggers",
    );
    let mut clauses: Vec<String> = Vec::new();
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(f) = filter {
        if let Some(k) = f.kind {
            clauses.push(format!("kind = ?{}", binds.len() + 1));
            binds.push(Box::new(k.as_wire().to_string()));
        }
        if let Some(e) = f.enabled {
            clauses.push(format!("enabled = ?{}", binds.len() + 1));
            binds.push(Box::new(if e { 1_i64 } else { 0_i64 }));
        }
        if let Some(sid) = &f.strategy_id {
            clauses.push(format!("strategy_id = ?{}", binds.len() + 1));
            binds.push(Box::new(sid.clone()));
        }
        if let Some(lid) = &f.strategy_lineage_id {
            clauses.push(format!("strategy_lineage_id = ?{}", binds.len() + 1));
            binds.push(Box::new(lid.clone()));
        }
    }
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    sql.push_str(" ORDER BY created_at DESC");

    let mut stmt = conn.prepare(&sql)?;
    let bind_refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(bind_refs.iter()), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::with_capacity(rows.len());
    for (id, sid, kind, enabled, last_fired_at, created_at, note, lineage) in rows {
        out.push(TriggerSummary {
            id,
            strategy_id: sid,
            kind: parse_kind(&kind)?,
            enabled: enabled != 0,
            last_fired_at,
            created_at,
            note,
            strategy_lineage_id: lineage,
        });
    }
    Ok(out)
}

pub(crate) fn get_by_id(conn: &Connection, id: &str) -> Result<Option<Trigger>, StateError> {
    conn.query_row(
        "SELECT id, strategy_id, kind, config_json, predicate_js, enabled,
                last_fired_at, created_at, dedup_window_ms, note, strategy_lineage_id
         FROM triggers WHERE id = ?1",
        params![id],
        |r| {
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
                r.get::<_, Option<String>>(9)?,
                r.get::<_, Option<String>>(10)?,
            ))
        },
    )
    .optional()?
    .map(|t| map_trigger_row(t.0, t.1, t.2, t.3, t.4, t.5, t.6, t.7, t.8, t.9, t.10))
    .transpose()
}

pub(crate) fn delete(conn: &Connection, id: &str) -> Result<bool, StateError> {
    // Hard-delete trigger_events first (FK), then trigger.
    conn.execute(
        "DELETE FROM trigger_events WHERE trigger_id = ?1",
        params![id],
    )?;
    let n = conn.execute("DELETE FROM triggers WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

pub(crate) fn set_enabled(
    conn: &Connection,
    id: &str,
    enabled: bool,
) -> Result<(), StateError> {
    let n = conn.execute(
        "UPDATE triggers SET enabled = ?1 WHERE id = ?2",
        params![if enabled { 1_i64 } else { 0_i64 }, id],
    )?;
    if n == 0 {
        return Err(StateError::NotFound(format!("trigger {id}")));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn record_event(
    conn: &Connection,
    trigger_id: &str,
    event_json: Option<&str>,
    run_id: Option<&str>,
    dedup_key: Option<&str>,
    skipped_reason: Option<&str>,
) -> Result<TriggerEvent, StateError> {
    let id = ulid::Ulid::new().to_string();
    let fired_at = now_rfc3339();
    conn.execute(
        "INSERT INTO trigger_events(id, trigger_id, event_json, fired_at,
                                     run_id, dedup_key, skipped_reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            &id,
            trigger_id,
            event_json,
            &fired_at,
            run_id,
            dedup_key,
            skipped_reason
        ],
    )?;
    // Update last_fired_at on the parent trigger when the event was a real fire
    // (not a skipped-by-dedup record). Caller decides via `skipped_reason`.
    if skipped_reason.is_none() {
        conn.execute(
            "UPDATE triggers SET last_fired_at = ?1 WHERE id = ?2",
            params![&fired_at, trigger_id],
        )?;
    }
    Ok(TriggerEvent {
        id,
        trigger_id: trigger_id.to_string(),
        event_json: event_json.map(|s| s.to_string()),
        fired_at,
        run_id: run_id.map(|s| s.to_string()),
        dedup_key: dedup_key.map(|s| s.to_string()),
        skipped_reason: skipped_reason.map(|s| s.to_string()),
    })
}

pub(crate) fn list_events(
    conn: &Connection,
    trigger_id: &str,
    limit: u64,
) -> Result<Vec<TriggerEvent>, StateError> {
    let mut stmt = conn.prepare(
        "SELECT id, trigger_id, event_json, fired_at, run_id, dedup_key, skipped_reason
         FROM trigger_events WHERE trigger_id = ?1
         ORDER BY fired_at DESC, id DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![trigger_id, limit as i64], |r| {
            Ok(TriggerEvent {
                id: r.get(0)?,
                trigger_id: r.get(1)?,
                event_json: r.get(2)?,
                fired_at: r.get(3)?,
                run_id: r.get(4)?,
                dedup_key: r.get(5)?,
                skipped_reason: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub(crate) fn check_dedup(
    conn: &Connection,
    trigger_id: &str,
    dedup_key: &str,
    window_ms: u64,
) -> Result<bool, StateError> {
    if window_ms == 0 {
        return Ok(false);
    }
    // Compare fired_at strings; RFC3339 lex order matches chronological order.
    let cutoff = chrono::Utc::now() - chrono::Duration::milliseconds(window_ms as i64);
    let cutoff_str = cutoff.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM trigger_events
         WHERE trigger_id = ?1 AND dedup_key = ?2 AND fired_at >= ?3",
        params![trigger_id, dedup_key, &cutoff_str],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}
