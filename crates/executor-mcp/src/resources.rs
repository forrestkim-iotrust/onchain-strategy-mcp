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
use executor_state::StateStore;
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

use crate::errors::{map_state_error, storage_error};

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
                "execution://{execution_id}",
                "execution",
                "Execution report with status and receipt. Populated in Phase 6.",
                "application/json",
            ),
            make_template(
                "journal://{execution_id}",
                "journal",
                "Journal entries for an execution. Populated in Phase 3+.",
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
    if uri.starts_with("execution://") {
        return Err(McpError::resource_not_found(
            format!("execution {uri} not found (Phase 6 wires runs)"),
            Some(json!({ "uri": uri, "phase": 6 })),
        ));
    }
    if uri.starts_with("journal://") {
        return Err(McpError::resource_not_found(
            format!("journal {uri} not found (Phase 3+ wires journal)"),
            Some(json!({ "uri": uri, "phase": 3 })),
        ));
    }
    Err(McpError::resource_not_found(
        format!("unsupported resource URI: {uri}"),
        Some(json!({ "uri": uri, "phase": 2 })),
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
