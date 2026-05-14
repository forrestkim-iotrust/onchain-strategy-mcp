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

// ─────────── Embedded examples + static docs (v1.3 self-documenting) ───────────
//
// `include_str!` bakes the example sources into the binary so the
// `examples://` resource family ships standalone — no on-disk dependency.

/// Embedded reference strategies, keyed by basename (filename without `.js`).
const EMBEDDED_STRATEGIES: &[(&str, &str)] = &[
    (
        "eth-funnel",
        include_str!("../../../examples/strategies/eth-funnel.js"),
    ),
    (
        "yield-snapshot",
        include_str!("../../../examples/strategies/yield-snapshot.js"),
    ),
    (
        "erc20-approve",
        include_str!("../../../examples/strategies/erc20-approve.js"),
    ),
    (
        "generic-counter-call",
        include_str!("../../../examples/strategies/generic-counter-call.js"),
    ),
];

/// Embedded reference contracts, keyed by basename (filename without `.sol`).
const EMBEDDED_CONTRACTS: &[(&str, &str)] = &[(
    "BatchExec",
    include_str!("../../../examples/contracts/BatchExec.sol"),
)];

const DOC_POLICY_MODEL: &str = r#"# Policy model

The runtime ships with a deny-by-default policy DSL loaded once at boot from
`.local/policy.toml`. Every action a strategy returns is checked against the
policy *before* signing. Anything not explicitly allowed is refused.

## Surface

- `signer` — the burner address actions execute from.
- `chains_allow` — list of allowed `chain_id`s. Out-of-list chains refuse.
- `contracts_allow` — per-contract allow list, each with:
  - `address`
  - `selectors_allow` — function 4-byte selectors (hex), or `*` for any
  - `value_cap_wei` — max native value per call, decimal string
- `erc20_caps` — per-token spend caps `{ token, spender, amount_cap }`
- `raw_call_allow_global` — when `false` (default), arbitrary low-level calls
  are refused; only `contractCall`/`erc20Approve` shapes pass.

## Minimal example

```toml
signer = "0x0000…dEaD"
chains_allow = [8453]
raw_call_allow_global = false

[[contracts_allow]]
address = "0xa238dd80c259a72e81d7e4664a9801593f98d1c5"  # Aave Pool on Base
selectors_allow = ["0x617ba037", "0x69328dec"]          # supply, withdraw
value_cap_wei = "0"

[[erc20_caps]]
token   = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"   # USDC on Base
spender = "0xa238dd80c259a72e81d7e4664a9801593f98d1c5"
amount_cap = "1000000000"                                 # 1000 USDC
```

`policy_get` returns the loaded view. `policy_update` is structurally
declared but returns `-32010 unimplemented` in this version — edit the TOML
and restart the server.
"#;

const DOC_EIP_7702: &str = r#"# EIP-7702 batching

When a strategy returns more than one action, the runtime auto-bundles them
into a single transaction via `BatchExec.executeBatch` invoked on the
sender's account through an EIP-7702 delegation. Either all actions land or
none do — there is no risk window between, e.g., an `approve` and the
`supply` that uses the allowance.

## Deterministic delegate address

The bundled `BatchExec` delegate is deployed via CREATE2 to:

    0x821fd81668823A3c5a65E95CeD5F050Ee54a4f53

The address is identical on every EVM chain because the CREATE2 deployer +
init code are pinned (see `executor_signer::predicted_delegate_address`).

If `[aa].delegate` is unset in `.local/config.toml`, the runtime auto-resolves
to this address. Override only if you know what you're doing.

## Deploying the delegate on a new chain

The runtime verifies code-at-delegate on the first 7702 batch attempt. If
empty, the action surfaces as `-32017 delegate_missing`. Fix:

    npx onchain-strategy-mcp deploy-delegate --chain <chain_id>

This deploys the BatchExec bytecode through the CREATE2 deployer using the
local burner — one-time, ~50k gas. The result lands at the deterministic
address above.

## When batching does NOT engage

- Single-action runs sign directly from the burner (no delegate involved).
- If `[aa].delegate` resolves but `ctx.evm.code` (via `evm_view`) at it is empty, batching fails
  fast (does NOT silently downgrade to sequential).

## When you need a custom delegate

If you point `[aa].delegate` at a contract you wrote, make sure it exposes
`executeBatch(Call[] calls)` AND a `receive()` so native transfers to the
delegated EOA succeed. The bundled `BatchExec` (see
`examples://contracts/BatchExec`) is the reference.
"#;

const DOC_TRIGGER_MODEL: &str = r#"# Trigger model

A trigger answers *when does a strategy run?*. Without one, you invoke
`strategy_run` by hand. Registered via `trigger_register`, attached to a
strategy id; events flow through an in-process dispatcher into the same
`strategy_run` pipeline.

## Kinds

| kind     | fires when                                              | required config              |
|----------|----------------------------------------------------------|------------------------------|
| manual   | An agent / user calls `strategy_run` directly            | none                         |
| interval | Every N ms (cron-style)                                  | `interval_ms`                |
| log      | Confirmed log matches address + topic(s) filter          | `address`, `topics[]`        |
| mempool  | Pending tx matches predicate on watched WSS node         | `predicate`, mempool WSS url |

Reserved (wired in upcoming versions): `block`, `webhook`.

## Concurrency

A trigger that fires while a previous run of the same strategy is still in
flight is rejected, not queued. The skip is journaled as a
`dedup_rejected` event readable via `trigger-events://{trigger_id}`. Build
strategies to be idempotent across closely-spaced fires.

## Examples

- **Funnel (inbound-fund detection):** `log` on the ERC20 contract filtered
  by `topics = [Transfer, *, burner]`. Catches *confirmed* deposits — avoids
  the reorg races mempool would introduce.

- **Oracle reaction:** `log` on the oracle aggregator address filtered by the
  price-update event topic. Strategy reads the new price via
  `ctx.evm.readContract` and decides.

- **Periodic snapshot:** `interval` with `interval_ms` matching the rate of
  change (hourly = `3_600_000`).

- **Front-running / pre-confirmation:** `mempool` is the only kind that sees
  unconfirmed txs. Requires `[trigger].mempool_wss_url` (Alchemy or
  equivalent). Without it, mempool workers warn-log and stay idle.

## Inspecting

- `trigger_list` — all registered triggers, filterable by kind / enabled.
- `trigger_get` / `trigger://{id}` — full row including config + predicate.
- `trigger_events` / `trigger-events://{id}` — last 100 events with outcome.
- `trigger_set_enabled({trigger_id, enabled})` — toggle without losing config.
"#;

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
            make_template(
                "trigger://{trigger_id}",
                "trigger",
                "v1.2 Trigger Core: returns the full Trigger row (kind, config_json, predicate, enabled, ...).",
                "application/json",
            ),
            make_template(
                "trigger-events://{trigger_id}",
                "trigger-events",
                "v1.2 Trigger Core: most recent 100 trigger events (fired, skipped, dedup-rejected) for the trigger.",
                "application/json",
            ),
            make_template(
                "examples://strategies",
                "example-strategies-index",
                "List of bundled reference strategies (JSON: { names: [...] }). Read each via examples://strategies/{name}.",
                "application/json",
            ),
            make_template(
                "examples://strategies/{name}",
                "example-strategy",
                "Embedded reference strategy source (JavaScript). Name is the filename without `.js` (eth-funnel, yield-snapshot, erc20-approve, generic-counter-call).",
                "application/javascript",
            ),
            make_template(
                "examples://contracts/{name}",
                "example-contract",
                "Embedded reference contract source (Solidity). Includes `BatchExec` — the EIP-7702 delegate.",
                "text/plain",
            ),
            make_template(
                "docs://policy-model",
                "docs-policy-model",
                "Concise prose: the deny-by-default policy DSL — allowed chains, contracts, selectors, value caps, ERC20 spend caps, with a minimal example.",
                "text/markdown",
            ),
            make_template(
                "docs://eip-7702",
                "docs-eip-7702",
                "Concise prose: how multi-action runs auto-bundle via EIP-7702, the deterministic CREATE2 BatchExec address, and the deploy-delegate flow.",
                "text/markdown",
            ),
            make_template(
                "docs://trigger-model",
                "docs-trigger-model",
                "Concise prose: when to use each trigger kind, with concrete examples (mirrors the `trigger_patterns` prompt for tools that prefer resources).",
                "text/markdown",
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
    if let Some(tid) = uri.strip_prefix("trigger-events://") {
        let tid = tid.to_string();
        return read_trigger_events(uri, tid, state).await;
    }
    if let Some(tid) = uri.strip_prefix("trigger://") {
        let tid = tid.to_string();
        return read_trigger(uri, tid, state).await;
    }
    // v1.3 self-documenting surface.
    if uri == "examples://strategies" {
        return Ok(read_examples_index(uri));
    }
    if let Some(name) = uri.strip_prefix("examples://strategies/") {
        return read_embedded(
            uri.clone(),
            name,
            EMBEDDED_STRATEGIES,
            "application/javascript",
        );
    }
    if let Some(name) = uri.strip_prefix("examples://contracts/") {
        return read_embedded(uri.clone(), name, EMBEDDED_CONTRACTS, "text/plain");
    }
    if let Some(doc) = static_doc_for(&uri) {
        return Ok(ReadResourceResult::new(vec![
            ResourceContents::text(doc.to_string(), uri).with_mime_type("text/markdown"),
        ]));
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

async fn read_trigger(
    uri: String,
    id: String,
    state: Arc<tokio::sync::Mutex<StateStore>>,
) -> Result<ReadResourceResult, McpError> {
    let id_owned = id.clone();
    let row = tokio::task::spawn_blocking(move || {
        let store = state.blocking_lock();
        store.get_trigger(&id_owned)
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
    .map_err(map_state_error)?;

    match row {
        None => Err(McpError::resource_not_found(
            format!("trigger {uri} not found"),
            Some(json!({ "uri": uri })),
        )),
        Some(t) => {
            let body = serde_json::to_string(&t)
                .map_err(|e| storage_error(format!("serialize trigger: {e}")))?;
            Ok(ReadResourceResult::new(vec![
                ResourceContents::text(body, uri).with_mime_type("application/json"),
            ]))
        }
    }
}

async fn read_trigger_events(
    uri: String,
    id: String,
    state: Arc<tokio::sync::Mutex<StateStore>>,
) -> Result<ReadResourceResult, McpError> {
    let id_owned = id.clone();
    let events = tokio::task::spawn_blocking(move || {
        let store = state.blocking_lock();
        store.list_trigger_events(&id_owned, 100)
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
    .map_err(map_state_error)?;

    let body = serde_json::to_string(&json!({ "events": events }))
        .map_err(|e| storage_error(format!("serialize trigger events: {e}")))?;
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(body, uri).with_mime_type("application/json"),
    ]))
}

// ─────────── v1.3 self-documenting resource handlers ───────────

fn read_examples_index(uri: String) -> ReadResourceResult {
    let names: Vec<&str> = EMBEDDED_STRATEGIES.iter().map(|(n, _)| *n).collect();
    let body = json!({ "names": names }).to_string();
    ReadResourceResult::new(vec![
        ResourceContents::text(body, uri).with_mime_type("application/json"),
    ])
}

fn read_embedded(
    uri: String,
    name: &str,
    table: &[(&str, &str)],
    mime: &str,
) -> Result<ReadResourceResult, McpError> {
    match table.iter().find(|(n, _)| *n == name) {
        Some((_, src)) => Ok(ReadResourceResult::new(vec![
            ResourceContents::text((*src).to_string(), uri).with_mime_type(mime),
        ])),
        None => {
            let known: Vec<&str> = table.iter().map(|(n, _)| *n).collect();
            Err(McpError::resource_not_found(
                format!("unknown embedded resource: {uri}"),
                Some(json!({ "uri": uri, "known": known })),
            ))
        }
    }
}

fn static_doc_for(uri: &str) -> Option<&'static str> {
    match uri {
        "docs://policy-model" => Some(DOC_POLICY_MODEL),
        "docs://eip-7702" => Some(DOC_EIP_7702),
        "docs://trigger-model" => Some(DOC_TRIGGER_MODEL),
        _ => None,
    }
}

#[cfg(test)]
mod self_documenting_resource_tests {
    use super::*;

    #[test]
    fn embedded_strategies_nonempty() {
        for (name, src) in EMBEDDED_STRATEGIES {
            assert!(!src.trim().is_empty(), "embedded strategy {name} is empty");
            assert!(
                src.contains("ctx."),
                "embedded strategy {name} should reference the ctx API"
            );
        }
    }

    #[test]
    fn embedded_contracts_nonempty() {
        for (name, src) in EMBEDDED_CONTRACTS {
            assert!(!src.trim().is_empty(), "embedded contract {name} is empty");
        }
    }

    #[test]
    fn static_docs_resolve() {
        assert!(static_doc_for("docs://policy-model").is_some());
        assert!(static_doc_for("docs://eip-7702").is_some());
        assert!(static_doc_for("docs://trigger-model").is_some());
        assert!(static_doc_for("docs://nope").is_none());
    }

    #[test]
    fn examples_index_lists_known_names() {
        let res = read_examples_index("examples://strategies".to_string());
        let txt = match &res.contents[0] {
            ResourceContents::TextResourceContents { text, .. } => text.clone(),
            _ => panic!("expected text contents"),
        };
        assert!(txt.contains("yield-snapshot"));
        assert!(txt.contains("eth-funnel"));
    }
}
