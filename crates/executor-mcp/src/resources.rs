//! Phase 1 resource surface — declares the three URI template shapes
//! (`strategy://{strategy_id}`, `execution://{execution_id}`,
//! `journal://{execution_id}`) that future phases will populate (D-03).
//!
//! Phase 1 never stores objects, so `list_resources` returns an empty array and
//! `read_resource` always yields a structured `resource_not_found` (-32002)
//! error with `data.phase = 1` so agents can tell "Phase 1 placeholder" apart
//! from "real miss" once Phase 2+ lands.
//!
//! ## `ResourceTemplate` construction (Plan 02/03 RESOLVED #5)
//!
//! On rmcp 1.5, `ResourceTemplate` is `Annotated<RawResourceTemplate>`:
//!
//! ```text
//! pub struct Annotated<T> { pub raw: T, pub annotations: Option<Annotations> }
//! pub struct RawResourceTemplate {
//!     pub uri_template: String,
//!     pub name: String,
//!     pub title: Option<String>,
//!     pub description: Option<String>,
//!     pub mime_type: Option<String>,
//!     pub icons: Option<Vec<Icon>>,
//! }
//! ```
//!
//! Neither struct implements `Default`, so the plan's `..Default::default()`
//! pattern is infeasible. We use the sanctioned builder chain
//! `RawResourceTemplate::new(..).with_description(..).with_mime_type(..)` and
//! then wrap with `Annotated::new(raw, None)` (Fallback 2 of the PLAN — see
//! SUMMARY for the exact field set adopted).

use rmcp::{
    ErrorData as McpError, RoleServer,
    model::{
        Annotated, ListResourceTemplatesResult, ListResourcesResult, PaginatedRequestParams,
        RawResourceTemplate, ReadResourceRequestParams, ReadResourceResult, ResourceTemplate,
    },
    service::RequestContext,
};
use serde_json::json;

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
    // Phase 1: empty. Phase 2+ populates from the state repository once
    // strategies / executions / journals exist.
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
                "Registered strategy (source + metadata). Populated in Phase 2.",
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
) -> Result<ReadResourceResult, McpError> {
    // Phase 1: always not-found. URI scheme parsing / path sanitization is
    // deliberately skipped (T-01-03-01) and lands with the real state repo in
    // Phase 2+. Until then every read answers the same shape so agents can
    // detect Phase-1 placeholder surface via `data.phase == 1`.
    Err(McpError::resource_not_found(
        "resource not found (Phase 1 placeholder surface — no objects stored yet)",
        Some(json!({ "uri": request.uri, "phase": 1 })),
    ))
}
