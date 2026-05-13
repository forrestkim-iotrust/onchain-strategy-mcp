//! v1.2 Trigger Core CRUD.
//!
//! - `id = hex(sha256(strategy_id || kind || config_json || predicate_js))` —
//!   content-addressed; same input ⇒ same id ⇒ idempotent register.
//! - Hard delete (no soft delete — triggers are runtime config, cleaner to drop).
//! - All SQL parameterised.
//! - `list` projects an explicit column set excluding `config_json` / `predicate_js`
//!   so list responses stay small.

use crate::error::StateError;
use crate::store::StateStore;
use executor_core::schema::trigger::{
    RegisterTriggerInput, Trigger, TriggerEvent, TriggerKind, TriggerListFilter, TriggerSummary,
};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn canonicalize_config(value: &serde_json::Value) -> String {
    // serde_json::to_string preserves object insertion order; for content-addressing
    // we sort object keys recursively so logically-equal configs hash identically.
    fn norm(v: &serde_json::Value) -> serde_json::Value {
        match v {
            serde_json::Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                let mut out = serde_json::Map::new();
                for k in keys {
                    out.insert(k.clone(), norm(&m[k]));
                }
                serde_json::Value::Object(out)
            }
            serde_json::Value::Array(a) => {
                serde_json::Value::Array(a.iter().map(norm).collect())
            }
            other => other.clone(),
        }
    }
    serde_json::to_string(&norm(value)).unwrap_or_else(|_| "{}".into())
}

pub fn hash_trigger(
    strategy_id: &str,
    kind: TriggerKind,
    config_json: &str,
    predicate_js: Option<&str>,
) -> String {
    let mut h = Sha256::new();
    h.update(strategy_id.as_bytes());
    h.update(b"|");
    h.update(kind.as_str().as_bytes());
    h.update(b"|");
    h.update(config_json.as_bytes());
    h.update(b"|");
    if let Some(p) = predicate_js {
        h.update(p.as_bytes());
    }
    hex::encode(h.finalize())
}

fn map_trigger_row(
    id: String,
    strategy_id: String,
    kind_raw: String,
    config_json: String,
    predicate: Option<String>,
    enabled_int: i64,
    last_fired_at: Option<String>,
    created_at: String,
    dedup_window_ms: Option<i64>,
) -> Result<Trigger, StateError> {
    let kind = TriggerKind::from_db_str(&kind_raw)
        .ok_or_else(|| StateError::Storage(format!("unknown trigger kind: {kind_raw}")))?;
    Ok(Trigger {
        id,
        strategy_id,
        kind,
        config_json,
        predicate,
        enabled: enabled_int != 0,
        last_fired_at,
        created_at,
        dedup_window_ms: dedup_window_ms.map(|v| v as u64),
    })
}

fn map_summary_row(
    id: String,
    strategy_id: String,
    kind_raw: String,
    enabled_int: i64,
    last_fired_at: Option<String>,
    created_at: String,
    dedup_window_ms: Option<i64>,
) -> Result<TriggerSummary, StateError> {
    let kind = TriggerKind::from_db_str(&kind_raw)
        .ok_or_else(|| StateError::Storage(format!("unknown trigger kind: {kind_raw}")))?;
    Ok(TriggerSummary {
        id,
        strategy_id,
        kind,
        enabled: enabled_int != 0,
        last_fired_at,
        created_at,
        dedup_window_ms: dedup_window_ms.map(|v| v as u64),
    })
}

fn register_trigger_inner(
    conn: &Connection,
    input: RegisterTriggerInput,
) -> Result<Trigger, StateError> {
    let config_json = canonicalize_config(&input.config);
    let id = hash_trigger(
        &input.strategy_id,
        input.kind,
        &config_json,
        input.predicate.as_deref(),
    );

    if let Some(existing) = get_trigger_inner(conn, &id)? {
        return Ok(existing);
    }

    let now = now_rfc3339();
    let dedup = input.dedup_window_ms.map(|v| v as i64);
    conn.execute(
        "INSERT INTO triggers(id, strategy_id, kind, config_json, predicate_js, enabled, last_fired_at, created_at, dedup_window_ms)
           VALUES (?1, ?2, ?3, ?4, ?5, 1, NULL, ?6, ?7)",
        params![
            &id,
            &input.strategy_id,
            input.kind.as_str(),
            &config_json,
            &input.predicate,
            &now,
            dedup,
        ],
    )?;

    Ok(Trigger {
        id,
        strategy_id: input.strategy_id,
        kind: input.kind,
        config_json,
        predicate: input.predicate,
        enabled: true,
        last_fired_at: None,
        created_at: now,
        dedup_window_ms: input.dedup_window_ms,
    })
}

fn get_trigger_inner(conn: &Connection, id: &str) -> Result<Option<Trigger>, StateError> {
    let row: Option<(
        String,
        String,
        String,
        String,
        Option<String>,
        i64,
        Option<String>,
        String,
        Option<i64>,
    )> = conn
        .query_row(
            "SELECT id, strategy_id, kind, config_json, predicate_js, enabled, last_fired_at, created_at, dedup_window_ms \
             FROM triggers WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            },
        )
        .optional()?;

    match row {
        None => Ok(None),
        Some((id, sid, kind, cfg, pred, en, lf, ca, dw)) => {
            Ok(Some(map_trigger_row(id, sid, kind, cfg, pred, en, lf, ca, dw)?))
        }
    }
}

fn list_triggers_inner(
    conn: &Connection,
    filter: Option<TriggerListFilter>,
) -> Result<Vec<TriggerSummary>, StateError> {
    let f = filter.unwrap_or_default();
    let mut sql = String::from(
        "SELECT id, strategy_id, kind, enabled, last_fired_at, created_at, dedup_window_ms \
         FROM triggers WHERE 1=1",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(k) = f.kind {
        sql.push_str(" AND kind = ?");
        args.push(Box::new(k.as_str().to_string()));
    }
    if let Some(en) = f.enabled {
        sql.push_str(" AND enabled = ?");
        args.push(Box::new(if en { 1_i64 } else { 0_i64 }));
    }
    if let Some(sid) = f.strategy_id {
        sql.push_str(" AND strategy_id = ?");
        args.push(Box::new(sid));
    }
    sql.push_str(" ORDER BY created_at DESC");

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(param_refs.iter()), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<i64>>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    rows.into_iter()
        .map(|(id, sid, k, en, lf, ca, dw)| map_summary_row(id, sid, k, en, lf, ca, dw))
        .collect()
}

fn delete_trigger_inner(conn: &Connection, id: &str) -> Result<bool, StateError> {
    let n = conn.execute("DELETE FROM triggers WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

fn set_trigger_enabled_inner(
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
fn record_trigger_event_inner(
    conn: &Connection,
    trigger_id: &str,
    event_json: Option<&str>,
    fired_at: &str,
    run_id: Option<&str>,
    dedup_key: Option<&str>,
    skipped_reason: Option<&str>,
) -> Result<TriggerEvent, StateError> {
    let id = ulid::Ulid::new().to_string();
    conn.execute(
        "INSERT INTO trigger_events(id, trigger_id, event_json, fired_at, run_id, dedup_key, skipped_reason) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            &id,
            trigger_id,
            event_json,
            fired_at,
            run_id,
            dedup_key,
            skipped_reason
        ],
    )?;

    // Bump last_fired_at when actually fired (run_id present OR no skipped_reason).
    if skipped_reason.is_none() {
        conn.execute(
            "UPDATE triggers SET last_fired_at = ?1 WHERE id = ?2",
            params![fired_at, trigger_id],
        )?;
    }

    Ok(TriggerEvent {
        id,
        trigger_id: trigger_id.to_string(),
        event_json: event_json.map(|s| s.to_string()),
        fired_at: fired_at.to_string(),
        run_id: run_id.map(|s| s.to_string()),
        dedup_key: dedup_key.map(|s| s.to_string()),
        skipped_reason: skipped_reason.map(|s| s.to_string()),
    })
}

fn list_trigger_events_inner(
    conn: &Connection,
    trigger_id: &str,
    limit: u64,
) -> Result<Vec<TriggerEvent>, StateError> {
    let mut stmt = conn.prepare(
        "SELECT id, trigger_id, event_json, fired_at, run_id, dedup_key, skipped_reason \
         FROM trigger_events WHERE trigger_id = ?1 \
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

fn check_dedup_inner(
    conn: &Connection,
    trigger_id: &str,
    dedup_key: &str,
    window_ms: u64,
) -> Result<bool, StateError> {
    let cutoff =
        chrono::Utc::now() - chrono::Duration::milliseconds(window_ms as i64);
    let cutoff_str = cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM trigger_events \
         WHERE trigger_id = ?1 AND dedup_key = ?2 AND fired_at >= ?3",
        params![trigger_id, dedup_key, &cutoff_str],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

// ---- StateStore façade ----

impl StateStore {
    pub fn register_trigger(
        &mut self,
        input: RegisterTriggerInput,
    ) -> Result<Trigger, StateError> {
        register_trigger_inner(&self.conn, input)
    }

    pub fn list_triggers(
        &self,
        filter: Option<TriggerListFilter>,
    ) -> Result<Vec<TriggerSummary>, StateError> {
        list_triggers_inner(&self.conn, filter)
    }

    pub fn get_trigger(&self, id: &str) -> Result<Option<Trigger>, StateError> {
        get_trigger_inner(&self.conn, id)
    }

    pub fn delete_trigger(&mut self, id: &str) -> Result<bool, StateError> {
        delete_trigger_inner(&self.conn, id)
    }

    pub fn set_trigger_enabled(
        &mut self,
        id: &str,
        enabled: bool,
    ) -> Result<(), StateError> {
        set_trigger_enabled_inner(&self.conn, id, enabled)
    }

    pub fn record_trigger_event(
        &mut self,
        trigger_id: &str,
        event_json: Option<&str>,
        fired_at: &str,
        run_id: Option<&str>,
        dedup_key: Option<&str>,
        skipped_reason: Option<&str>,
    ) -> Result<TriggerEvent, StateError> {
        record_trigger_event_inner(
            &self.conn,
            trigger_id,
            event_json,
            fired_at,
            run_id,
            dedup_key,
            skipped_reason,
        )
    }

    pub fn list_trigger_events(
        &self,
        trigger_id: &str,
        limit: u64,
    ) -> Result<Vec<TriggerEvent>, StateError> {
        list_trigger_events_inner(&self.conn, trigger_id, limit)
    }

    pub fn check_dedup(
        &self,
        trigger_id: &str,
        dedup_key: &str,
        window_ms: u64,
    ) -> Result<bool, StateError> {
        check_dedup_inner(&self.conn, trigger_id, dedup_key, window_ms)
    }
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StateStore;
    use tempfile::TempDir;

    fn fresh_store() -> (TempDir, StateStore) {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("state.db");
        let store = StateStore::open(&path).expect("open");
        (dir, store)
    }

    fn seed_strategy(store: &mut StateStore, name: &str) -> String {
        let source = format!("// strategy {name}\nasync function run() {{ return 'noop'; }}");
        let outcome = store
            .register_strategy(name, &source, None, None)
            .expect("register strategy");
        match outcome {
            crate::strategies::RegisterOutcome::Created(s) => s.id,
            crate::strategies::RegisterOutcome::AlreadyExists(s) => s.id,
        }
    }

    fn base_input(strategy_id: &str) -> RegisterTriggerInput {
        RegisterTriggerInput {
            strategy_id: strategy_id.to_string(),
            kind: TriggerKind::Interval,
            config: serde_json::json!({ "interval_ms": 1000 }),
            predicate: None,
            dedup_window_ms: None,
        }
    }

    #[test]
    fn register_trigger_is_idempotent_on_same_input() {
        let (_d, mut store) = fresh_store();
        let sid = seed_strategy(&mut store, "s1");

        let t1 = store.register_trigger(base_input(&sid)).expect("t1");
        let t2 = store.register_trigger(base_input(&sid)).expect("t2");
        assert_eq!(t1.id, t2.id, "same input ⇒ same id");
        assert_eq!(t1.created_at, t2.created_at, "row not overwritten");

        // Different config ⇒ different id.
        let mut diff = base_input(&sid);
        diff.config = serde_json::json!({ "interval_ms": 2000 });
        let t3 = store.register_trigger(diff).expect("t3");
        assert_ne!(t1.id, t3.id);

        // Key-order-insensitive: same logical config ⇒ same id.
        let mut reordered = base_input(&sid);
        reordered.config = serde_json::json!({ "interval_ms": 1000, "extra": null });
        let t4 = store
            .register_trigger(reordered.clone())
            .expect("t4");
        // Different because "extra" added, but reordering same keys:
        let mut reordered2 = reordered.clone();
        // Build a fresh map in inverted insertion order:
        reordered2.config = {
            let mut m = serde_json::Map::new();
            m.insert("extra".into(), serde_json::Value::Null);
            m.insert("interval_ms".into(), serde_json::json!(1000));
            serde_json::Value::Object(m)
        };
        let t4b = store.register_trigger(reordered2).expect("t4b");
        assert_eq!(t4.id, t4b.id, "object key order must not affect hash");
    }

    #[test]
    fn list_triggers_filters_by_kind_and_enabled() {
        let (_d, mut store) = fresh_store();
        let sid = seed_strategy(&mut store, "s1");

        let interval = store.register_trigger(base_input(&sid)).unwrap();
        let mut manual_in = base_input(&sid);
        manual_in.kind = TriggerKind::Manual;
        manual_in.config = serde_json::json!({});
        let manual = store.register_trigger(manual_in).unwrap();

        // disable the manual one
        store.set_trigger_enabled(&manual.id, false).unwrap();

        let all = store.list_triggers(None).unwrap();
        assert_eq!(all.len(), 2);

        let only_interval = store
            .list_triggers(Some(TriggerListFilter {
                kind: Some(TriggerKind::Interval),
                ..Default::default()
            }))
            .unwrap();
        assert_eq!(only_interval.len(), 1);
        assert_eq!(only_interval[0].id, interval.id);

        let only_enabled = store
            .list_triggers(Some(TriggerListFilter {
                enabled: Some(true),
                ..Default::default()
            }))
            .unwrap();
        assert_eq!(only_enabled.len(), 1);
        assert_eq!(only_enabled[0].id, interval.id);

        let only_disabled = store
            .list_triggers(Some(TriggerListFilter {
                enabled: Some(false),
                ..Default::default()
            }))
            .unwrap();
        assert_eq!(only_disabled.len(), 1);
        assert_eq!(only_disabled[0].id, manual.id);

        let by_sid = store
            .list_triggers(Some(TriggerListFilter {
                strategy_id: Some(sid.clone()),
                ..Default::default()
            }))
            .unwrap();
        assert_eq!(by_sid.len(), 2);
    }

    #[test]
    fn delete_trigger_removes_row() {
        let (_d, mut store) = fresh_store();
        let sid = seed_strategy(&mut store, "s1");
        let t = store.register_trigger(base_input(&sid)).unwrap();

        let deleted = store.delete_trigger(&t.id).unwrap();
        assert!(deleted);
        assert!(store.get_trigger(&t.id).unwrap().is_none());

        let again = store.delete_trigger(&t.id).unwrap();
        assert!(!again, "second delete reports no row removed");
    }

    #[test]
    fn set_trigger_enabled_toggles() {
        let (_d, mut store) = fresh_store();
        let sid = seed_strategy(&mut store, "s1");
        let t = store.register_trigger(base_input(&sid)).unwrap();
        assert!(t.enabled);

        store.set_trigger_enabled(&t.id, false).unwrap();
        let after = store.get_trigger(&t.id).unwrap().unwrap();
        assert!(!after.enabled);

        store.set_trigger_enabled(&t.id, true).unwrap();
        let after2 = store.get_trigger(&t.id).unwrap().unwrap();
        assert!(after2.enabled);

        let err = store.set_trigger_enabled("nonexistent", true);
        assert!(matches!(err, Err(StateError::NotFound(_))));
    }

    #[test]
    fn record_trigger_event_inserts_row() {
        let (_d, mut store) = fresh_store();
        let sid = seed_strategy(&mut store, "s1");
        let t = store.register_trigger(base_input(&sid)).unwrap();

        let fired = now_rfc3339();
        let ev = store
            .record_trigger_event(
                &t.id,
                Some(r#"{"kind":"interval"}"#),
                &fired,
                Some("run-abc"),
                None,
                None,
            )
            .unwrap();
        assert_eq!(ev.trigger_id, t.id);
        assert_eq!(ev.run_id.as_deref(), Some("run-abc"));

        let list = store.list_trigger_events(&t.id, 10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, ev.id);

        // last_fired_at bumped for non-skipped event.
        let reread = store.get_trigger(&t.id).unwrap().unwrap();
        assert_eq!(reread.last_fired_at.as_deref(), Some(fired.as_str()));

        // A skipped event does not bump last_fired_at.
        let later = now_rfc3339();
        store
            .record_trigger_event(
                &t.id,
                None,
                &later,
                None,
                Some("dk-1"),
                Some("dedup"),
            )
            .unwrap();
        let reread2 = store.get_trigger(&t.id).unwrap().unwrap();
        assert_eq!(
            reread2.last_fired_at.as_deref(),
            Some(fired.as_str()),
            "skipped event must not update last_fired_at"
        );
    }

    #[test]
    fn check_dedup_returns_true_within_window_false_outside() {
        let (_d, mut store) = fresh_store();
        let sid = seed_strategy(&mut store, "s1");
        let t = store.register_trigger(base_input(&sid)).unwrap();

        // Insert an event with a fired_at far in the past.
        let old_ts = (chrono::Utc::now() - chrono::Duration::seconds(10))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        store
            .record_trigger_event(&t.id, None, &old_ts, Some("r1"), Some("k1"), None)
            .unwrap();

        // 1s window: no match (event is 10s old).
        assert!(!store.check_dedup(&t.id, "k1", 1_000).unwrap());

        // 60s window: should match.
        assert!(store.check_dedup(&t.id, "k1", 60_000).unwrap());

        // Different dedup_key never matches.
        assert!(!store.check_dedup(&t.id, "other", 60_000).unwrap());
    }
}
