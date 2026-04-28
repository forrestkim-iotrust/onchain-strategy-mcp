//! Resource surface — declares the three URI template shapes
//! (`strategy://{strategy_id}`, `execution://{execution_id}`,
//! `journal://{execution_id}`).
//!
//! Phase 1 returned `resource_not_found` for every read. Phase 2 wires
//! `strategy://{id}` to the live `StateStore`: malformed ids and unknown
//! rows still surface as `-32002 resource_not_found`, but a known id now
//! returns the full `StrategyGetResponse` JSON body. `execution://` and
//! `journal://` keep returning the structured phase-gated `not_found`
//! envelope (Phase 3+ / 6+).
//!
//! ## `ResourceTemplate` construction
//!
//! On rmcp 1.5, `ResourceTemplate` is `Annotated<RawResourceTemplate>`. We
//! use `RawResourceTemplate::new(..).with_description(..).with_mime_type(..)`
//! and then wrap with `Annotated::new(raw, None)` (Plan 01-03 PLAN RESOLVED #5
//! Fallback 2).

use std::sync::Arc;

use executor_core::schema::strategy::StrategyGetResponse;
use executor_state::{StateError, StateStore};
use rmcp::{
    ErrorData as McpError, RoleServer,
    model::{
        Annotated, ListResourceTemplatesResult, ListResourcesResult, PaginatedRequestParams,
        RawResourceTemplate, ReadResourceRequestParams, ReadResourceResult, ResourceContents,
        ResourceTemplate,
    },
    service::RequestContext,
};
use serde_json::json;

use crate::{
    errors::{map_state_error, storage_error},
    tools::build_execution_report,
};

fn make_template(
    uri_template: &str,
    name: &str,
    description: &str,
    mime_type: &str,
) -> ResourceTemplate {
    let raw = RawResourceTemplate::new(uri_template, name)
        .with_description(description)
        .with_mime_type(mime_type);
    Annotated::new(raw, None)
}

pub(crate) async fn list_resources_impl(
    _req: Option<PaginatedRequestParams>,
    _ctx: RequestContext<RoleServer>,
) -> Result<ListResourcesResult, McpError> {
    // Phase 2: stay empty. Enumerating all strategies here would duplicate
    // `strategy_list`; agents who want the catalogue should use the tool.
    Ok(ListResourcesResult {
        meta: None,
        next_cursor: None,
        resources: Vec::new(),
    })
}

pub(crate) async fn list_resource_templates_impl(
    _req: Option<PaginatedRequestParams>,
    _ctx: RequestContext<RoleServer>,
) -> Result<ListResourceTemplatesResult, McpError> {
    Ok(ListResourceTemplatesResult {
        meta: None,
        next_cursor: None,
        resource_templates: vec![
            make_template(
                "strategy://{strategy_id}",
                "strategy",
                "Registered strategy (source + metadata). Live in Phase 2.",
                "application/json",
            ),
            make_template(
                "execution://{run_id}",
                "execution",
                "Receipt-backed execution report for the run ID returned by strategy_run.",
                "application/json",
            ),
            make_template(
                "journal://{run_id}",
                "journal",
                "Populated in Phase 3 (returns source_reads + actions + logs for the run).",
                "application/json",
            ),
        ],
    })
}

pub(crate) async fn read_resource_impl(
    request: ReadResourceRequestParams,
    _ctx: RequestContext<RoleServer>,
    state: Arc<tokio::sync::Mutex<StateStore>>,
) -> Result<ReadResourceResult, McpError> {
    let uri = request.uri.clone();

    // Branch on URI scheme. Only `strategy://{id}` is wired in Phase 2.
    if let Some(id) = uri.strip_prefix("strategy://") {
        let id_owned = id.to_string();
        return read_strategy(uri, id_owned, state).await;
    }
    if let Some(rid) = uri.strip_prefix("journal://") {
        let rid_owned = rid.to_string();
        return read_journal(uri, rid_owned, state).await;
    }
    if let Some(run_id) = uri.strip_prefix("execution://") {
        let run_id = run_id.to_string();
        return read_execution(uri, run_id, state).await;
    }
    Err(McpError::resource_not_found(
        format!("unsupported resource URI: {uri}"),
        Some(json!({ "uri": uri, "phase": 3 })),
    ))
}

async fn read_strategy(
    uri: String,
    id: String,
    state: Arc<tokio::sync::Mutex<StateStore>>,
) -> Result<ReadResourceResult, McpError> {
    // D-09a at the resource boundary: reject malformed ids before hitting
    // the DB. Mirrors `validation::validate_strategy_id_format` but surfaces
    // as resource_not_found (-32002) per the resources/read contract.
    if id.len() != 64 || !id.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)) {
        return Err(McpError::resource_not_found(
            format!("malformed strategy id in uri: {uri}"),
            Some(json!({ "uri": uri, "code": "malformed_id" })),
        ));
    }

    let id_owned = id.clone();
    let row = tokio::task::spawn_blocking(move || {
        let store = state.blocking_lock();
        store.get_strategy_by_id(&id_owned)
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
    .map_err(map_state_error)?;

    match row {
        None => Err(McpError::resource_not_found(
            format!("strategy {uri} not found"),
            Some(json!({ "uri": uri })),
        )),
        Some(s) => {
            let resp = StrategyGetResponse {
                strategy_id: s.id,
                name: s.name,
                source: s.source,
                description: s.description,
                tags: s.tags,
                created_at: s.created_at,
                deleted_at: s.deleted_at,
            };
            let body = serde_json::to_string(&resp)
                .map_err(|e| storage_error(format!("serialize strategy: {e}")))?;
            Ok(ReadResourceResult::new(vec![
                ResourceContents::text(body, uri).with_mime_type("application/json"),
            ]))
        }
    }
}

async fn read_execution(
    uri: String,
    run_id: String,
    state: Arc<tokio::sync::Mutex<StateStore>>,
) -> Result<ReadResourceResult, McpError> {
    validate_run_resource_id(&uri, &run_id)?;
    let report = build_execution_report(state, run_id).await.map_err(|err| {
        if err.code.0 == -32014 {
            McpError::resource_not_found(format!("run {uri} not found"), Some(json!({ "uri": uri })))
        } else {
            err
        }
    })?;
    let body = serde_json::to_string(&report)
        .map_err(|e| storage_error(format!("serialize execution: {e}")))?;
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(body, uri).with_mime_type("application/json"),
    ]))
}

fn validate_run_resource_id(uri: &str, run_id: &str) -> Result<(), McpError> {
    // Boundary check: ULID is 26 chars, alphanumeric (Crockford). Permissive
    // shape check matches the Phase-2 strategy:// posture.
    if run_id.len() != 26 || !run_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(McpError::resource_not_found(
            format!("malformed run id in uri: {uri}"),
            Some(json!({ "uri": uri, "code": "malformed_id" })),
        ));
    }
    Ok(())
}

async fn read_journal(
    uri: String,
    run_id: String,
    state: Arc<tokio::sync::Mutex<StateStore>>,
) -> Result<ReadResourceResult, McpError> {
    validate_run_resource_id(&uri, &run_id)?;

    let rid_owned = run_id.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<_, StateError> {
        let store = state.blocking_lock();
        let exists = store.get_run(&rid_owned)?;
        if exists.is_none() {
            return Ok(None);
        }
        let s = store.list_source_reads_for_run(&rid_owned)?;
        let a = store.list_actions_for_run(&rid_owned)?;
        let l = store.list_logs_for_run(&rid_owned)?;
        let d = store.list_decisions_for_run(&rid_owned)?;
        Ok(Some((s, a, l, d)))
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
    .map_err(map_state_error)?;

    let (sources, actions, logs, decisions) = match result {
        Some(t) => t,
        None => {
            return Err(McpError::resource_not_found(
                format!("run {uri} not found"),
                Some(json!({ "uri": uri })),
            ));
        }
    };

    // Build action rows. Use serde_json::to_value for the outcome enum so
    // we get the canonical snake_case wire string — NEVER format!("{:?}",..)
    // (would yield "simulationfailure" instead of "simulation_failure").
    let mut action_rows = Vec::with_capacity(actions.len());
    for a in &actions {
        let outcome_val = serde_json::to_value(a.outcome)
            .map_err(|e| storage_error(format!("serialize outcome: {e}")))?;
        action_rows.push(serde_json::json!({
            "id": a.id,
            "outcome": outcome_val,
            "payload_json": a.payload_json,
            "recorded_at": a.recorded_at,
        }));
    }

    let body = serde_json::json!({
        "run_id": run_id,
        "source_reads": sources.iter().map(|s| serde_json::json!({
            "id": s.id,
            "kind": s.kind,
            "target": s.target,
            "payload_json": s.payload_json,
            "recorded_at": s.recorded_at,
        })).collect::<Vec<_>>(),
        "actions": action_rows,
        "decisions": decisions.iter().map(|d| serde_json::json!({
            "id": d.id,
            "run_id": d.run_id,
            "action_index": d.action_index,
            "gate": d.gate,
            "verdict": d.verdict,
            "rule": d.rule,
            "detail": d.detail,
            "payload_json": d.payload_json,
            "recorded_at": d.recorded_at,
            "seq": d.seq,
        })).collect::<Vec<_>>(),
        "logs": logs.iter().map(|l| serde_json::json!({
            "id": l.id,
            "message": l.message,
            "recorded_at": l.recorded_at,
        })).collect::<Vec<_>>(),
    });
    let body_text = serde_json::to_string(&body)
        .map_err(|e| storage_error(format!("serialize journal: {e}")))?;
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(body_text, uri).with_mime_type("application/json"),
    ]))
}
