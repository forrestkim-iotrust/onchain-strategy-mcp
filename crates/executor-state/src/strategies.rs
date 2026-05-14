//! Content-addressed strategy CRUD (D-01..D-02, D-07a..c).
//!
//! - `id = hex(sha256(source))` — source-only hash, name/metadata excluded
//!   (D-01a). Same source ⇒ same id, idempotent register (D-01b).
//! - Name uniqueness is enforced **only among non-deleted rows** via the
//!   partial unique index defined in [`crate::schema`] (D-01c).
//! - All SQL parameterised — no `format!`-into-SQL anywhere (T-02-01-01).
//! - `list` projects an explicit column set so `source` is never copied
//!   into list responses (T-02-01-03 / D-07a).

use crate::error::StateError;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct Strategy {
    pub id: String,
    pub name: String,
    pub source: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub created_at: String,
    pub deleted_at: Option<String>,
    /// v1.4 strategy bundle: canonical JSON for the `records` schema.
    /// NULL for legacy (pre-v1.4) registrations.
    pub records_json: Option<String>,
    /// v1.4 strategy bundle: JS source for the `view` function. NULL for
    /// legacy registrations; consumers fall back to a generic balance view.
    pub view_source: Option<String>,
    /// v1.5 Track 1B: cached static extraction of contracts/selectors the
    /// strategy touches. Canonical JSON; see
    /// `crates/executor-mcp/src/contracts_touched.rs` for the shape. NULL on
    /// rows registered before v1.5.
    pub contracts_touched_json: Option<String>,
    /// v1.8 name-anchored lineage: stable identifier preserved across
    /// re-registrations of the same `name` even when content changes. Equal
    /// to `id` for legacy rows (backfill); equal to the original register-
    /// time mint for any version >= 1 of a fresh lineage.
    pub lineage_id: String,
    /// v1.10 named actions: canonical JSON of `{name → JS source}`. NULL when
    /// the bundle declared no actions. Folded into the id hash.
    pub actions_json: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StrategySummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub created_at: String,
    pub deleted_at: Option<String>,
    /// True when the row carries v1.4 records schema or view source. Used by
    /// callers to know whether `strategy://{id}/view` returns bundle output
    /// vs. generic fallback.
    pub has_bundle: bool,
    /// v1.5 Track 1C: cached static extraction so list-time policy alignment
    /// can be batched against a single policy snapshot without N+1 row reads.
    /// NULL on rows registered before v1.5.
    pub contracts_touched_json: Option<String>,
    /// v1.8 name-anchored lineage: stable across version bumps.
    pub lineage_id: String,
    /// v1.8: 1-based position within the lineage (1 = first registered,
    /// 2 = second iteration of the name, ...). Computed at list time from
    /// the lineage's full history (active + deleted rows).
    pub version: u32,
    /// v1.10: names of the bundle's manual-only `actions[*]` entries, sorted.
    /// Empty / NULL when the bundle declared none. Surfaced so list-time
    /// resources can advertise per-strategy named actions without dragging
    /// the full JS source into the response.
    pub action_names: Vec<String>,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum RegisterOutcome {
    Created(Strategy),
    /// Same-source idempotent path (D-01b). Carries the **existing** row so
    /// the caller can surface `existing_name` / `existing_description` /
    /// `existing_tags` in the agent-facing response.
    AlreadyExists(Strategy),
    /// v1.8 name-anchored lineage: a previously active row with the same
    /// `name` had different content; it was soft-deleted and a new version
    /// inserted with the SAME `lineage_id`. Triggers, runs, and records
    /// attached to that lineage automatically follow.
    ReplacedVersion {
        /// The freshly inserted row (new id, same lineage_id, deleted_at
        /// is None).
        created: Strategy,
        /// The prior active version that was soft-deleted as part of this
        /// register call.
        previous: Strategy,
        /// 1-based position of the new row in its lineage (e.g. 2 means
        /// "second register of this name").
        new_version: u32,
        /// 1-based position of the row that was just superseded.
        previous_version: u32,
        /// Whether the bundle's execute (source) bytes changed across the
        /// version bump.
        execute_changed: bool,
        /// Whether `records_json` changed across the bump (either side may
        /// have been NULL).
        records_changed: bool,
        /// Whether `view_source` changed across the bump (either side may
        /// have been NULL).
        view_changed: bool,
        /// v1.10: whether the bundle's `actions` map changed across the
        /// version bump. Either side may have been NULL (no actions).
        actions_changed: bool,
    },
}

impl RegisterOutcome {
    /// Convenience: borrow the resulting active strategy regardless of
    /// which variant we landed in. For `Created` and `ReplacedVersion`,
    /// this is the freshly inserted row; for `AlreadyExists`, it is the
    /// pre-existing row that was content-addressed.
    pub fn active_strategy(&self) -> &Strategy {
        match self {
            RegisterOutcome::Created(s) => s,
            RegisterOutcome::AlreadyExists(s) => s,
            RegisterOutcome::ReplacedVersion { created, .. } => created,
        }
    }

    /// Same as [`Self::active_strategy`] but consumes self.
    pub fn into_active_strategy(self) -> Strategy {
        match self {
            RegisterOutcome::Created(s) => s,
            RegisterOutcome::AlreadyExists(s) => s,
            RegisterOutcome::ReplacedVersion { created, .. } => created,
        }
    }
}

/// Content-addressed strategy id.
///
/// Back-compat invariant: when no bundle fields are present (legacy
/// pre-v1.4 register call), this returns the same hash as the v1.0 form
/// (`sha256(source)`). Existing strategy ids therefore stay stable across
/// the v1.4 upgrade.
///
/// When `records_json` and/or `view_source` is present, the hash mixes
/// them in with explicit length-prefixed framing so that distinct bundle
/// trios collide-free even if one component is empty.
pub fn hash_bundle(
    source: &str,
    records_json: Option<&str>,
    view_source: Option<&str>,
) -> String {
    if records_json.is_none() && view_source.is_none() {
        // Legacy path: source-only hash, byte-for-byte compatible with v1.0..v1.3.
        let mut h = Sha256::new();
        h.update(source.as_bytes());
        return hex::encode(h.finalize());
    }
    // Bundle path: length-prefixed concat of three sections.
    let mut h = Sha256::new();
    h.update(b"osmcp-bundle-v1\n");
    write_section(&mut h, "execute", source);
    write_section(&mut h, "records", records_json.unwrap_or(""));
    write_section(&mut h, "view", view_source.unwrap_or(""));
    hex::encode(h.finalize())
}

/// v1.8 name-anchored lineage: per-lineage content hash that folds in the
/// lineage_id so two unrelated lineages that happen to share the same
/// `execute + records + view` content still get distinct ids.
///
/// v1.10: an optional `actions_json` (canonical JSON of named actions) is
/// folded in as well. When NULL it is treated as the empty string — same
/// byte-shape as a bundle with no actions, so back-compat with v1.8/v1.9
/// hashes is preserved iff the caller passes `None` for actions_json.
///
/// Back-compat invariant: when `lineage_id` is `None`, this is
/// byte-identical to [`hash_bundle`] — legacy rows backfilled with
/// `lineage_id = id` would recurse, so the legacy path skips the lineage
/// mix-in entirely. Fresh lineages (v1.8+) always pass `Some(ulid)`.
pub fn hash_bundle_with_lineage(
    lineage_id: Option<&str>,
    source: &str,
    records_json: Option<&str>,
    view_source: Option<&str>,
    actions_json: Option<&str>,
) -> String {
    let Some(lin) = lineage_id else {
        // Legacy lineage anchor: ignore actions_json so byte-identical
        // pre-v1.10 hashes are preserved. Callers that want actions folded
        // in MUST pass a fresh (v1.8+) lineage anchor.
        return hash_bundle(source, records_json, view_source);
    };
    let mut h = Sha256::new();
    // v1.8 frame tag is retained when there are no actions so pre-v1.10
    // bundles keep their existing ids. v1.10 bumps to a new tag the moment
    // a bundle declares actions so the hash domain is unambiguous.
    if actions_json.is_some() {
        h.update(b"osmcp-bundle-v1.10\n");
    } else {
        h.update(b"osmcp-bundle-v1.8\n");
    }
    write_section(&mut h, "lineage", lin);
    write_section(&mut h, "execute", source);
    write_section(&mut h, "records", records_json.unwrap_or(""));
    write_section(&mut h, "view", view_source.unwrap_or(""));
    if let Some(a) = actions_json {
        write_section(&mut h, "actions", a);
    }
    hex::encode(h.finalize())
}

fn write_section(h: &mut Sha256, tag: &str, body: &str) {
    h.update(tag.as_bytes());
    h.update(b":");
    h.update((body.len() as u64).to_be_bytes());
    h.update(b"\n");
    h.update(body.as_bytes());
    h.update(b"\n");
}

/// Legacy alias retained so existing callers (and tests) keep compiling.
/// Same byte-for-byte output as `hash_bundle(source, None, None)`.
pub fn hash_source(source: &str) -> String {
    hash_bundle(source, None, None)
}

pub(crate) fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn encode_tags(tags: Option<&[String]>) -> Option<String> {
    tags.map(|t| serde_json::to_string(t).unwrap_or_else(|_| "[]".into()))
}

fn decode_tags(raw: Option<String>) -> Option<Vec<String>> {
    raw.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
}

/// v1.10: extract the (sorted) action names from a canonical actions_json
/// blob. Returns an empty Vec for NULL / invalid JSON so list-time
/// projections never panic. Sort order is BTreeMap-natural ASCII, matching
/// the canonical write path.
fn decode_action_names(raw: Option<&str>) -> Vec<String> {
    let Some(s) = raw else { return Vec::new() };
    let Ok(map) = serde_json::from_str::<std::collections::BTreeMap<String, serde_json::Value>>(s)
    else {
        return Vec::new();
    };
    map.into_keys().collect()
}

#[allow(clippy::too_many_arguments)]
fn map_strategy(
    id: String,
    name: String,
    source: String,
    description: Option<String>,
    tags_raw: Option<String>,
    created_at: String,
    deleted_at: Option<String>,
    records_json: Option<String>,
    view_source: Option<String>,
    contracts_touched_json: Option<String>,
    lineage_id: Option<String>,
    actions_json: Option<String>,
) -> Strategy {
    // Backfill defense: a row created on a pre-migration binary then read
    // on a post-migration binary should always see `lineage_id = id`. The
    // schema `migrate()` runs at open time so this should never be NULL in
    // practice, but the fallback keeps the typed surface lineage-aware
    // even if a future code path forgets to set it.
    let lineage_id = lineage_id.unwrap_or_else(|| id.clone());
    Strategy {
        id,
        name,
        source,
        description,
        tags: decode_tags(tags_raw),
        created_at,
        deleted_at,
        records_json,
        view_source,
        contracts_touched_json,
        lineage_id,
        actions_json,
    }
}

/// Mint a fresh lineage identifier. Uses ULID for chronological sortability
/// AND collision resistance — the lineage id appears in the strategy id
/// hash for fresh lineages, so two `strategy_register` calls with identical
/// content but different names (or different processes) still produce
/// distinct ids by construction.
fn mint_lineage_id() -> String {
    ulid::Ulid::new().to_string()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn register(
    conn: &Connection,
    name: &str,
    source: &str,
    description: Option<&str>,
    tags: Option<&[String]>,
    records_json: Option<&str>,
    view_source: Option<&str>,
    contracts_touched_json: Option<&str>,
    actions_json: Option<&str>,
) -> Result<RegisterOutcome, StateError> {
    // v1.5 Track 1B: `contracts_touched_json` is a DERIVATION from `source`
    // (the regex extractor in `executor-mcp::contracts_touched` computes it)
    // and is INTENTIONALLY excluded from the id hash. Re-deriving never
    // changes the id; only execute/records/view do.
    //
    // v1.8 name-anchored lineage: id derivation now depends on lineage_id
    // for fresh lineages. The case matrix:
    //
    //  | active row for `name`      | resolution                                |
    //  |----------------------------|-------------------------------------------|
    //  | none                       | mint new lineage_id, fold into hash       |
    //  | content hash matches       | idempotent — return existing row          |
    //  | content differs            | soft-delete old, insert new w/ same       |
    //  |                            | lineage_id, fold lineage_id into NEW id   |
    //
    // The legacy / backward-compat path: existing rows have lineage_id = id
    // (backfilled by `migrate()`), so re-registering an EXACT byte-identical
    // pre-v1.8 bundle returns AlreadyExists without re-hashing.

    // Step 1: try to find an active row for this name.
    let active_for_name = get_by_name(conn, name)?;

    // Step 2: figure out the effective lineage_id and content hash.
    let (effective_lineage, id) = match &active_for_name {
        Some(active) => {
            // Active row exists. Inherit its lineage. Use the v1.8 lineage-
            // folded hash IF the existing row was itself minted under the
            // v1.8 scheme (i.e. its lineage_id differs from its id). For
            // legacy rows (lineage_id == id), keep computing the legacy
            // hash — this is what preserves the exact-byte idempotency of
            // re-registering an unchanged pre-v1.8 bundle.
            let lineage = active.lineage_id.clone();
            let id = if lineage == active.id && actions_json.is_none() {
                // Legacy lineage anchor with no actions — keep legacy hash so
                // byte-identical re-register of a pre-v1.10 bundle is still
                // an AlreadyExists short-circuit. The moment a legacy lineage
                // adopts actions, the hash domain shifts (v1.10 frame tag) —
                // that's the intended version bump.
                hash_bundle(source, records_json, view_source)
            } else {
                hash_bundle_with_lineage(
                    Some(&lineage),
                    source,
                    records_json,
                    view_source,
                    actions_json,
                )
            };
            (lineage, id)
        }
        None => {
            // Fresh lineage: mint a ULID and fold it into the id hash.
            let lineage = mint_lineage_id();
            let id = hash_bundle_with_lineage(
                Some(&lineage),
                source,
                records_json,
                view_source,
                actions_json,
            );
            (lineage, id)
        }
    };

    // Step 3: hash collision short-circuit. A row with this exact id is
    //   - the same lineage's current active version → idempotent
    //   - some other (soft-deleted or differently-named) row that just
    //     happens to share the hash. The lineage-folded id hash makes the
    //     latter astronomically unlikely for v1.8+ rows, but legacy rows
    //     can collide on byte-identical source; still treat as idempotent
    //     since the row is content-addressed.
    if let Some(existing) = get_by_id(conn, &id)? {
        return Ok(RegisterOutcome::AlreadyExists(existing));
    }

    // Step 4: branch on active_for_name.
    let now = now_rfc3339();
    let tags_json = encode_tags(tags);

    match active_for_name {
        None => {
            // Fresh lineage, no name conflict. Plain INSERT.
            conn.execute(
                "INSERT INTO strategies(id, name, source, description, tags, created_at, records_json, view_source, contracts_touched_json, lineage_id, actions_json)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![&id, name, source, description, tags_json, &now, records_json, view_source, contracts_touched_json, &effective_lineage, actions_json],
            )?;

            Ok(RegisterOutcome::Created(Strategy {
                id,
                name: name.to_string(),
                source: source.to_string(),
                description: description.map(|s| s.to_string()),
                tags: tags.map(|t| t.to_vec()),
                created_at: now,
                deleted_at: None,
                records_json: records_json.map(|s| s.to_string()),
                view_source: view_source.map(|s| s.to_string()),
                contracts_touched_json: contracts_touched_json.map(|s| s.to_string()),
                lineage_id: effective_lineage,
                actions_json: actions_json.map(|s| s.to_string()),
            }))
        }
        Some(previous) => {
            // Same-name re-register with different content. Soft-delete the
            // old row, INSERT the new one with the SAME lineage_id. Triggers
            // / runs / records attached to this lineage automatically follow
            // (their FK is strategy_lineage_id, not strategy_id).

            // Compute version numbers BEFORE mutating so the response is
            // deterministic w.r.t. the row counts we observed.
            let prev_version = lineage_version_count(conn, &effective_lineage)?;
            let new_version = prev_version.saturating_add(1);

            // Compare bundle parts so the response can flag scope of change.
            let execute_changed = previous.source != source;
            let records_changed = previous.records_json.as_deref() != records_json;
            let view_changed = previous.view_source.as_deref() != view_source;
            let actions_changed = previous.actions_json.as_deref() != actions_json;

            // Soft-delete BEFORE INSERT so the unique-on-name index
            // (`idx_strategies_name_active`) and the lineage-active index
            // (`idx_strategies_lineage_active`) are free for the new row.
            conn.execute(
                "UPDATE strategies SET deleted_at = ?1 \
                 WHERE id = ?2 AND deleted_at IS NULL",
                params![&now, &previous.id],
            )?;

            conn.execute(
                "INSERT INTO strategies(id, name, source, description, tags, created_at, records_json, view_source, contracts_touched_json, lineage_id, actions_json)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![&id, name, source, description, tags_json, &now, records_json, view_source, contracts_touched_json, &effective_lineage, actions_json],
            )?;

            // Re-read the soft-deleted previous row so the caller sees the
            // accurate `deleted_at` timestamp (vs. the cloned active copy).
            let previous_with_deletion = get_by_id(conn, &previous.id)?
                .unwrap_or(previous);

            Ok(RegisterOutcome::ReplacedVersion {
                created: Strategy {
                    id,
                    name: name.to_string(),
                    source: source.to_string(),
                    description: description.map(|s| s.to_string()),
                    tags: tags.map(|t| t.to_vec()),
                    created_at: now,
                    deleted_at: None,
                    records_json: records_json.map(|s| s.to_string()),
                    view_source: view_source.map(|s| s.to_string()),
                    contracts_touched_json: contracts_touched_json.map(|s| s.to_string()),
                    lineage_id: effective_lineage,
                    actions_json: actions_json.map(|s| s.to_string()),
                },
                previous: previous_with_deletion,
                new_version,
                previous_version: prev_version,
                execute_changed,
                records_changed,
                view_changed,
                actions_changed,
            })
        }
    }
}

/// Count how many rows belong to this lineage (active + soft-deleted).
/// Returned value is the "current version number" of the lineage's most
/// recent row — so an empty lineage returns 0, a lineage with one history
/// row returns 1, and so on.
pub(crate) fn lineage_version_count(
    conn: &Connection,
    lineage_id: &str,
) -> Result<u32, StateError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM strategies WHERE lineage_id = ?1",
        params![lineage_id],
        |r| r.get(0),
    )?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

pub(crate) fn list(
    conn: &Connection,
    include_deleted: bool,
) -> Result<Vec<StrategySummary>, StateError> {
    // Explicit column set — `source` is intentionally absent (T-02-01-03 / D-07a).
    // `records_json` and `view_source` are pulled as boolean presence flags
    // (so list responses can advertise `has_bundle` without dragging the JS
    // source through every listing).
    //
    // v1.8: include `lineage_id` so the caller can group by lineage; the
    // `version` field is the per-lineage rank of THIS row by created_at
    // ascending (1 = oldest in lineage, latest = N). We compute the rank
    // SQL-side via a correlated COUNT so the projection stays a single
    // statement.
    let sql = if include_deleted {
        "SELECT id, name, description, tags, created_at, deleted_at, \
                records_json IS NOT NULL OR view_source IS NOT NULL, \
                contracts_touched_json, \
                lineage_id, \
                (SELECT COUNT(*) FROM strategies s2 \
                 WHERE s2.lineage_id = strategies.lineage_id \
                   AND s2.created_at <= strategies.created_at) AS version, \
                actions_json \
         FROM strategies ORDER BY created_at DESC"
    } else {
        "SELECT id, name, description, tags, created_at, deleted_at, \
                records_json IS NOT NULL OR view_source IS NOT NULL, \
                contracts_touched_json, \
                lineage_id, \
                (SELECT COUNT(*) FROM strategies s2 \
                 WHERE s2.lineage_id = strategies.lineage_id \
                   AND s2.created_at <= strategies.created_at) AS version, \
                actions_json \
         FROM strategies WHERE deleted_at IS NULL ORDER BY created_at DESC"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map([], |r| {
            let lineage: Option<String> = r.get(8)?;
            let id: String = r.get(0)?;
            let version_i64: i64 = r.get(9)?;
            let actions_raw: Option<String> = r.get(10)?;
            Ok(StrategySummary {
                id: id.clone(),
                name: r.get(1)?,
                description: r.get(2)?,
                tags: decode_tags(r.get::<_, Option<String>>(3)?),
                created_at: r.get(4)?,
                deleted_at: r.get(5)?,
                has_bundle: r.get::<_, i64>(6)? != 0,
                contracts_touched_json: r.get(7)?,
                lineage_id: lineage.unwrap_or(id),
                version: u32::try_from(version_i64).unwrap_or(1).max(1),
                action_names: decode_action_names(actions_raw.as_deref()),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// v1.8: list all rows belonging to a lineage, newest-first. Includes
/// soft-deleted rows so the history view can show the full version chain.
pub(crate) fn list_for_lineage(
    conn: &Connection,
    lineage_id: &str,
) -> Result<Vec<StrategySummary>, StateError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, description, tags, created_at, deleted_at, \
                records_json IS NOT NULL OR view_source IS NOT NULL, \
                contracts_touched_json, \
                lineage_id, \
                (SELECT COUNT(*) FROM strategies s2 \
                 WHERE s2.lineage_id = strategies.lineage_id \
                   AND s2.created_at <= strategies.created_at) AS version, \
                actions_json \
         FROM strategies WHERE lineage_id = ?1 \
         ORDER BY created_at DESC",
    )?;
    let rows = stmt
        .query_map(params![lineage_id], |r| {
            let lineage: Option<String> = r.get(8)?;
            let id: String = r.get(0)?;
            let version_i64: i64 = r.get(9)?;
            let actions_raw: Option<String> = r.get(10)?;
            Ok(StrategySummary {
                id: id.clone(),
                name: r.get(1)?,
                description: r.get(2)?,
                tags: decode_tags(r.get::<_, Option<String>>(3)?),
                created_at: r.get(4)?,
                deleted_at: r.get(5)?,
                has_bundle: r.get::<_, i64>(6)? != 0,
                contracts_touched_json: r.get(7)?,
                lineage_id: lineage.unwrap_or(id),
                version: u32::try_from(version_i64).unwrap_or(1).max(1),
                action_names: decode_action_names(actions_raw.as_deref()),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// v1.8: fetch the current (single, not-deleted) version for a lineage.
/// The unique partial index `idx_strategies_lineage_active` makes "at most
/// one active per lineage" a schema-level invariant.
pub(crate) fn get_active_for_lineage(
    conn: &Connection,
    lineage_id: &str,
) -> Result<Option<Strategy>, StateError> {
    conn.query_row(
        "SELECT id, name, source, description, tags, created_at, deleted_at, \
                records_json, view_source, contracts_touched_json, lineage_id, \
                actions_json \
         FROM strategies \
         WHERE lineage_id = ?1 AND deleted_at IS NULL \
         LIMIT 1",
        params![lineage_id],
        |r| {
            Ok(map_strategy(
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
                r.get(10)?,
                r.get(11)?,
            ))
        },
    )
    .optional()
    .map_err(StateError::from)
}

/// v1.8: compute a row's 1-based `version` within its lineage. Cheap
/// helper for handlers that want to surface "this is version N" without
/// pulling the full `list` projection.
pub(crate) fn version_for_id(
    conn: &Connection,
    id: &str,
) -> Result<Option<u32>, StateError> {
    let row: Option<i64> = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM strategies s2 \
                     WHERE s2.lineage_id = s.lineage_id \
                       AND s2.created_at <= s.created_at) \
             FROM strategies s WHERE s.id = ?1",
            params![id],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    Ok(row.map(|v| u32::try_from(v).unwrap_or(1).max(1)))
}

pub(crate) fn get_by_id(conn: &Connection, id: &str) -> Result<Option<Strategy>, StateError> {
    conn.query_row(
        "SELECT id, name, source, description, tags, created_at, deleted_at, \
                records_json, view_source, contracts_touched_json, lineage_id, \
                actions_json \
         FROM strategies WHERE id = ?1 LIMIT 1",
        params![id],
        |r| {
            Ok(map_strategy(
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
                r.get(10)?,
                r.get(11)?,
            ))
        },
    )
    .optional()
    .map_err(StateError::from)
}

pub(crate) fn get_by_name(conn: &Connection, name: &str) -> Result<Option<Strategy>, StateError> {
    conn.query_row(
        "SELECT id, name, source, description, tags, created_at, deleted_at, \
                records_json, view_source, contracts_touched_json, lineage_id, \
                actions_json \
         FROM strategies WHERE name = ?1 AND deleted_at IS NULL",
        params![name],
        |r| {
            Ok(map_strategy(
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
                r.get(10)?,
                r.get(11)?,
            ))
        },
    )
    .optional()
    .map_err(StateError::from)
}

/// Sets `deleted_at` if not yet set; otherwise returns the existing
/// `deleted_at` unchanged (D-07c idempotent semantics).
pub(crate) fn soft_delete(conn: &Connection, id: &str) -> Result<String, StateError> {
    let existing: Option<Option<String>> = conn
        .query_row(
            "SELECT deleted_at FROM strategies WHERE id = ?1",
            params![id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?;

    match existing {
        None => Err(StateError::NotFound(format!("strategy {id}"))),
        Some(Some(ts)) => Ok(ts),
        Some(None) => {
            let now = now_rfc3339();
            conn.execute(
                "UPDATE strategies SET deleted_at = ?1 WHERE id = ?2",
                params![&now, id],
            )?;
            Ok(now)
        }
    }
}

pub(crate) fn is_deleted(conn: &Connection, id: &str) -> Result<Option<bool>, StateError> {
    let row: Option<Option<String>> = conn
        .query_row(
            "SELECT deleted_at FROM strategies WHERE id = ?1",
            params![id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?;
    Ok(row.map(|d| d.is_some()))
}
