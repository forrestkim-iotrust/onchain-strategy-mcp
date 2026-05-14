//! Resource surface — declares the three URI template shapes
//! (`strategy://{strategy_id}`, `execution://{run_id}`,
//! `journal://{run_id}`).
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

use executor_core::schema::execution::RunStatus;
use executor_core::schema::strategy::StrategyGetResponse;
use executor_core::schema::trigger::{TriggerKind, TriggerListFilter};
use executor_policy::LoadedPolicy;
use executor_state::{
    LIST_RUNS_DEFAULT_LIMIT, LIST_RUNS_LIMIT_CAP, RunListFilter, StateError, StateStore,
};
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
use tokio::sync::RwLock;

use crate::{
    errors::{invalid_params, map_state_error, storage_error},
    tools::{build_execution_report, strategy_to_get_response},
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

`policy_get` returns the loaded view. There is no `policy_update` tool —
edit `.local/policy.toml` directly and restart the server.
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

const DOC_STRATEGY_BUNDLE: &str = r#"# Strategy bundle (v1.4)

A strategy is registered as a **bundle** of up to three pieces. Only the
first is required.

| slot | required | role |
|------|----------|------|
| `execute` | yes | the action-producing JS function (existing v1.0+ behaviour) |
| `records` | no  | declarative capture schema — what to remember from confirmed actions |
| `view`    | no  | interpreter function the runtime calls when an agent reads `strategy://{id}/view` |

`strategy_id` is content-addressed as `sha256(execute + records + view)`.
A legacy single-function registration (no records/view) hashes identically
to its v1.0..v1.3 form, so existing ids stay stable across upgrades.

## execute

```js
(ctx) => Action[] | "noop"
```

Same as pre-v1.4. Returns onchain actions (`ctx.actions.contractCall`,
`ctx.actions.erc20Approve`) or the string `"noop"` for "no action this tick".

## records

Declarative capture spec. The runtime watches confirmed actions and, when
one matches a record's `on` clause, stores the evaluated `capture` map
into `strategy_records_capture`. Capture failures NEVER break the run —
they log a warning and skip the offending field.

```js
records: [
  {
    name: "supply",
    on: {
      kind: "contractCall",            // also: "erc20Approve", "log"
      target: "0xa238dd80...",          // optional address filter
      selector: "supply"                // function name OR 4-byte hex
    },
    capture: {
      amount_micro:    "args[1]",       // dotted accessor over args
      asset:           "args[0]",
      ts:              "tx.ts",
      tx_hash:         "tx.hash"
      // also supported:
      // logs.<EventName>[<self|0>].<field>
      // tx.{hash,block,ts,gas_used}
      // view.<helper>(args)             — runtime-provided named helpers
    }
  }
]
```

The capture DSL is intentionally narrow. If it can't express what you
need, use a tx_hash accessor and post-process in your `view` function.

## view

```js
(ctx, records) => any
```

Pure-read function. Called whenever `strategy://{id}/view` is requested.
`ctx` carries the same `evm.*` helpers as a strategy; `records` exposes
the captured rows aggregated host-side as `{ count, latest, each, sum(field) }`
per record name.

The runtime wraps the return value with an honesty envelope:
`{ data: <your return>, confidence: "full" | "partial" | "missing", reason?, remediation? }`.

Strategies without `view` get a generic fallback (burner balances only)
with `confidence: "missing"`.

## $assets — declaring user positions for portfolio aggregation

A view function MAY return a top-level `$assets` array. Each entry declares
one position the user holds at some venue. The web UI / portfolio resource
aggregates `$assets` across ALL active strategies to compute a unified
portfolio total.

Entries that don't appear in `$assets` are treated as **observations** and
rendered per-strategy only — they don't contribute to portfolio sums.

A strategy with no `$assets` (e.g. a pure-observer yield comparator)
contributes nothing to the portfolio total. That's correct behaviour.

### Required keys

| key       | type   | meaning |
|-----------|--------|---------|
| `chain_id`| number | chain id (e.g. 8453 for Base). Required for cross-chain dedup. |
| `venue`   | string | machine-identifier of the protocol/wrapper (e.g. `"aave-v3-base"`, `"curve-ve"`, `"idle"`). |
| `asset`   | string | display name of the asset (e.g. `"USDC"`, `"CRV locked"`). |
| `amount`  | string | human-readable balance (e.g. `"0.257164"`). |
| `raw`     | string | base-unit balance as a uint string (e.g. `"257164"`). |
| `decimals`| number | decimals used to convert `raw` → `amount`. |

### Optional keys

| key       | type   | meaning |
|-----------|--------|---------|
| `address` | string | the wrapper/position contract (aToken / cToken / LP token / etc). Used for explorer links. |
| `usd`     | number | USD valuation. Computed by the strategy; omit if unknown. UI shows amount-only when missing. |

### Authoring rules

- The view function reads the *current* onchain state with `ctx.evm.*`
  and writes the result into `$assets`. There is NO separate verifier —
  the view IS the verifier by construction.
- For ERC20-style positions: read `erc20Balance(wrapper, burner)`.
- For non-ERC20 (locked stakes, veToken, NFT positions, …): read the
  protocol-specific view function with `ctx.evm.readContract` and put
  the meaningful field into `raw`.
- If `usd` is unknowable (illiquid asset, no oracle), omit it. The UI
  will surface a "no USD valuation" note.

### Example (ERC20 — Aave aUSDC)

```js
view: (ctx, _records) => {
  const BURNER = "0xe32f0F034C544040D147F7094F223a9C61CDf23F";
  const AUSDC  = "0x4e65fE4DbA92790696d040ac24Aa414708F5c0AB";
  const bal    = ctx.evm.erc20Balance(AUSDC, BURNER, "pending");
  return {
    $assets: [
      {
        chain_id: 8453,
        venue:    "aave-v3-base",
        asset:    "USDC",
        address:  AUSDC,
        amount:   (Number(bal) / 1e6).toFixed(6),
        raw:      bal.toString(),
        decimals: 6,
        usd:      Number(bal) / 1e6
      }
    ],
    // anything else is observation — UI renders per-strategy, no portfolio sum
    activity: { /* ... */ }
  };
}
```

### Example (non-ERC20 — Curve veCRV locked amount)

```js
view: (ctx, _records) => {
  const VECRV = "0x5f3b...";
  const locked = ctx.evm.readContract({
    address:  VECRV,
    abi:      VECRV_ABI,                  // VotingEscrow.locked
    function: "locked",
    args:     [BURNER]
  });
  return {
    $assets: [
      {
        chain_id: 1,
        venue:    "curve-ve",
        asset:    "CRV locked",
        amount:   (Number(locked.amount) / 1e18).toFixed(4),
        raw:      locked.amount.toString(),
        decimals: 18,
        usd:      Number(locked.amount) / 1e18 * CRV_PRICE_USD  // author computes
      }
    ]
  };
}
```

## Automatic rendering hints (web UI v1.6+)

The web UI auto-renders view output. Following these conventions makes
the output prettier without any extra work:

| key suffix               | rendered as                              |
|--------------------------|------------------------------------------|
| `_usdc` / `_usd` / `_eth`/ `_wei` / `_micro` / `_pct` / `_bps` | numeric with the matching unit          |
| `_ts` / `_at` (RFC3339)  | "11 minutes ago" with absolute tooltip   |
| `_address` / `_tx_hash`  | shortened (...) with explorer link       |
| array of objects (same keys) | table |
| top-level scalar         | KPI card |

None of this is enforced; it's only display polish.

## Where to next

- `examples://strategies/eth-funnel-bundle` — the eth-funnel pattern as a
  full v1.4 bundle (execute + records + view + $assets).
- `docs://policy-model`, `docs://trigger-model`, `docs://eip-7702` —
  adjacent runtime docs.
- `strategy://{id}/view` resource — what the runtime returns when an
  agent reads a bundle's interpreted state.
- `strategy://{id}/records` resource — raw capture rows.
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
            // v1.4 Track B: summary-with-hyperlinks listing for the
            // 30-second-rule answer to "what is running?".
            make_template(
                "strategy://list",
                "strategy-list",
                "Active strategy summaries with inline `trigger_kinds`, `last_fire_at`, \
                 `last_24h` run rollup, `has_bundle` flag, and a `view_uri` follow-up. \
                 Query params: `status=active|deleted|all` (default `active`), `tag=<name>` \
                 (exact-match), `summary=true|false` (default `true`, embeds the rich rollup; \
                 `false` returns the bare summary fields only).",
                "application/json",
            ),
            make_template(
                "strategy://{strategy_id}",
                "strategy",
                "Registered strategy (source + metadata). Live in Phase 2.",
                "application/json",
            ),
            // v1.4 Track B: name-aliased lookup so human-friendly references work.
            make_template(
                "strategy://by-name/{name}",
                "strategy-by-name",
                "Resolve a strategy by its human-friendly active name. Returns the same \
                 shape as `strategy://{strategy_id}` for the active row. 404 when no \
                 active strategy carries that name.",
                "application/json",
            ),
            // v1.4 Track A4: bundle view + raw records browse.
            make_template(
                "strategy://{strategy_id}/view",
                "strategy-view",
                "v1.4 strategy bundle: returns the output of the strategy's `view(ctx, records)` \
                 function. Bundled strategies receive their captured records aggregated host-side \
                 (sum/count/latest/since/each on each record name) and run the view inside the same \
                 JS sandbox as `evm_view`. Strategies registered without a `view` source fall back to \
                 a generic balance snapshot wrapped with `confidence: \"missing\"` + remediation hint. \
                 Wrapped with the v1.4 honesty contract: `{ data, confidence, reason?, remediation? }`.",
                "application/json",
            ),
            make_template(
                "strategy://{strategy_id}/records",
                "strategy-records",
                "v1.4 strategy bundle: raw capture rows from `strategy_records_capture`. \
                 Newest-first, hard-capped at 500. Supports `since` (RFC3339 exclusive lower bound \
                 on captured_at). Example: `strategy://abc.../records?since=2026-05-14T00:00:00Z`. \
                 Use this for forensics / aggregate prototyping; for the strategy-defined \
                 interpretation, prefer `strategy://{strategy_id}/view`.",
                "application/json",
            ),
            // v1.4 Track B: filtered trigger listing.
            make_template(
                "trigger://list",
                "trigger-list",
                "Trigger summaries (id, strategy_id, kind, enabled, last_fired_at, created_at). \
                 Query params: `strategy_id=<id>` (exact match), `kind=manual|interval|log|mempool`, \
                 `enabled=true|false`. All filters AND together.",
                "application/json",
            ),
            // v1.4 Track B: policy snapshot resource.
            make_template(
                "policy://current",
                "policy-current",
                "Current loaded policy. Shape: `{ loaded: bool, policy?: <toml-projected JSON>, \
                 confidence?, reason?, remediation? }`. When `loaded` is false, the response \
                 carries `confidence: \"missing\"` + a `remediation` pointing at `.local/policy.toml`.",
                "application/json",
            ),
            make_template(
                "execution://{run_id}",
                "execution",
                "Receipt-backed execution report for the run ID returned by strategy_run.",
                "application/json",
            ),
            make_template(
                "execution://list",
                "execution-list",
                "Run summaries filtered by query string (newest-first). \
                 Supported parameters: `strategy_id` (exact match on runs.strategy_id), \
                 `since` (RFC3339 timestamp, exclusive lower bound on started_at), \
                 `status` (one of `succeeded` | `failed` | `noop`; `noop` matches runs \
                 whose journal recorded a noop outcome — i.e. RunStatus=Succeeded with a \
                 journal_actions row of outcome=noop), \
                 `limit` (default 50, hard cap 500). \
                 Example: `execution://list?strategy_id=ab12...&status=failed&limit=10`. \
                 Use this resource when you have a strategy id but no run id — the per-run \
                 `execution://{run_id}` resource is the next hop for any returned summary.",
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
            make_template(
                "docs://strategy-bundle",
                "docs-strategy-bundle",
                "v1.4 strategy bundle authoring guide — the canonical reference for `execute` + `records` + `view` shape, the records capture DSL, and the `$assets` convention for portfolio-aggregatable positions. Read this BEFORE registering any non-trivial strategy.",
                "text/markdown",
            ),
        ],
    })
}

pub(crate) async fn read_resource_impl(
    request: ReadResourceRequestParams,
    _ctx: RequestContext<RoleServer>,
    state: Arc<tokio::sync::Mutex<StateStore>>,
    policy: Arc<RwLock<Option<LoadedPolicy>>>,
) -> Result<ReadResourceResult, McpError> {
    let uri = request.uri.clone();

    // v1.4 Track B / A4 collision plan: `strategy://list` MUST match BEFORE
    // the generic `strategy://{id}` branch. Also keep space for A4's
    // `strategy://{id}/view` and `strategy://{id}/records` — those land in
    // `read_strategy_subresource` after the prefix-stripped id is parsed.
    if uri == "strategy://list" {
        return read_strategy_list(uri, String::new(), state).await;
    }
    if let Some(query) = uri.strip_prefix("strategy://list?") {
        let q = query.to_string();
        return read_strategy_list(uri, q, state).await;
    }
    if let Some(name) = uri.strip_prefix("strategy://by-name/") {
        let name = name.to_string();
        return read_strategy_by_name(uri, name, state).await;
    }
    // v1.4 Track A4: bundle view + raw records browse.
    // Match `strategy://{id}/view` and `strategy://{id}/records[?...]` BEFORE
    // the generic `strategy://{id}` branch below.
    if let Some(rest) = uri.strip_prefix("strategy://") {
        if let Some((id, tail)) = rest.split_once('/') {
            if tail == "view" {
                return read_strategy_view(uri.clone(), id.to_string(), state).await;
            }
            if let Some(query) = tail.strip_prefix("records?") {
                return read_strategy_records(uri.clone(), id.to_string(), query.to_string(), state).await;
            }
            if tail == "records" {
                return read_strategy_records(uri.clone(), id.to_string(), String::new(), state).await;
            }
        }
    }
    // policy://current — v1.4 Track B. Match exactly to avoid shadowing.
    if uri == "policy://current" {
        return read_policy_current(uri, policy).await;
    }
    // trigger://list[?...] — v1.4 Track B. Must match before generic
    // `trigger://{id}` branch.
    if uri == "trigger://list" {
        return read_trigger_list(uri, String::new(), state).await;
    }
    if let Some(query) = uri.strip_prefix("trigger://list?") {
        let q = query.to_string();
        return read_trigger_list(uri, q, state).await;
    }

    // Generic strategy://{id} (after the above v1.4 specializations).
    if let Some(id) = uri.strip_prefix("strategy://") {
        let id_owned = id.to_string();
        return read_strategy(uri, id_owned, state).await;
    }
    if let Some(rid) = uri.strip_prefix("journal://") {
        let rid_owned = rid.to_string();
        return read_journal(uri, rid_owned, state).await;
    }
    // v1.4 Track C: `execution://list[?...]` MUST match BEFORE the
    // generic `execution://{run_id}` branch.
    if uri == "execution://list" {
        return read_execution_list(uri, String::new(), state).await;
    }
    if let Some(query) = uri.strip_prefix("execution://list?") {
        let q = query.to_string();
        return read_execution_list(uri, q, state).await;
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

// ─────────── v1.4 Track C — execution://list ───────────
//
// URI: `execution://list?strategy_id=...&since=ISO8601&status=succeeded|failed|noop&limit=N`
//
// Filters: all four are optional and AND together. Empty filter → most-recent
// 50 runs across all strategies. The handler enforces the v1.4 contract
// guarantees:
//
// - Invalid `since` (not RFC3339) ⇒ `invalid_params` error, NOT a silent zero-row
//   response. Agents shouldn't have to guess whether `since` was filtered out.
// - Invalid `status` (not one of the three) ⇒ `invalid_params` error.
// - `limit` defaults to 50; values >500 are hard-capped (state layer enforces).
// - `limit` that fails to parse as `u64` ⇒ `invalid_params` error.
// - Empty result set ⇒ `{ runs: [], count: 0 }` (success), NOT an error.
//
// The response shape:
// ```json
// {
//   "runs": [ { run_id, strategy_id, status, started_at, finished_at, action_count }, ... ],
//   "count": N,
//   "filters_applied": { strategy_id?, since?, status?, limit }
// }
// ```
//
// `filters_applied` echoes the parsed filter values (after defaulting/capping)
// so agents can confirm what the server actually used — critical when the
// answer is "0 runs" and the agent needs to debug whether its filter was
// misinterpreted.

async fn read_execution_list(
    uri: String,
    query: String,
    state: Arc<tokio::sync::Mutex<StateStore>>,
) -> Result<ReadResourceResult, McpError> {
    let parsed = parse_execution_list_query(&query)?;
    let ParsedExecutionListQuery {
        strategy_id,
        since,
        status_label,
        status_run,
        journal_outcome,
        limit,
    } = parsed;

    let filter = RunListFilter {
        strategy_id: strategy_id.clone(),
        since: since.clone(),
        status: status_run,
        journal_outcome: journal_outcome.clone(),
        limit: Some(limit),
    };

    let summaries = tokio::task::spawn_blocking(move || {
        let store = state.blocking_lock();
        store.list_runs(&filter)
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
    .map_err(map_state_error)?;

    let runs_json: Vec<serde_json::Value> = summaries
        .iter()
        .map(|s| {
            let status_wire = serde_json::to_value(s.status).unwrap_or(json!("unknown"));
            json!({
                "run_id": s.run_id,
                "strategy_id": s.strategy_id,
                "status": status_wire,
                "started_at": s.started_at,
                "finished_at": s.finished_at,
                "action_count": s.action_count,
            })
        })
        .collect();

    // Echo the effective filter set so agents can confirm what was applied.
    // `status` is the wire label the caller passed (or `null`) — NOT the
    // internal RunStatus mapping — so it round-trips cleanly.
    let mut filters_applied = serde_json::Map::new();
    if let Some(sid) = &strategy_id {
        filters_applied.insert("strategy_id".to_string(), json!(sid));
    }
    if let Some(s) = &since {
        filters_applied.insert("since".to_string(), json!(s));
    }
    if let Some(s) = &status_label {
        filters_applied.insert("status".to_string(), json!(s));
    }
    filters_applied.insert("limit".to_string(), json!(limit));

    let count = runs_json.len();
    let body = json!({
        "runs": runs_json,
        "count": count,
        "filters_applied": serde_json::Value::Object(filters_applied),
    });
    let body_text = serde_json::to_string(&body)
        .map_err(|e| storage_error(format!("serialize execution list: {e}")))?;
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(body_text, uri).with_mime_type("application/json"),
    ]))
}

struct ParsedExecutionListQuery {
    strategy_id: Option<String>,
    since: Option<String>,
    /// The wire label the caller passed (e.g. `"succeeded"`), used for the
    /// `filters_applied` echo in the response.
    status_label: Option<String>,
    /// Mapped `RunStatus` when `status_label` is `succeeded` or `failed`.
    status_run: Option<RunStatus>,
    /// Mapped `journal_actions.outcome` wire string when `status_label` is
    /// `noop` — see [`RunListFilter::journal_outcome`].
    journal_outcome: Option<String>,
    /// Effective limit after defaulting / capping.
    limit: u64,
}

fn parse_execution_list_query(qs: &str) -> Result<ParsedExecutionListQuery, McpError> {
    let mut strategy_id: Option<String> = None;
    let mut since: Option<String> = None;
    let mut status_label: Option<String> = None;
    let mut limit_raw: Option<String> = None;

    if !qs.is_empty() {
        for pair in qs.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (key, value) = match pair.split_once('=') {
                Some((k, v)) => (k, v),
                None => (pair, ""),
            };
            // Minimal percent-decoding — query values are typically clean
            // (hex ids, RFC3339, integers). Reject `+` as space (form-encoding
            // doesn't apply to URI query strings; `%20` is the spec form).
            let value = percent_decode(value).map_err(|e| {
                invalid_params(format!(
                    "execution://list: malformed percent-encoding in `{key}`: {e}"
                ))
            })?;
            match key {
                "strategy_id" => strategy_id = Some(value),
                "since" => since = Some(value),
                "status" => status_label = Some(value),
                "limit" => limit_raw = Some(value),
                other => {
                    return Err(invalid_params(format!(
                        "execution://list: unknown query parameter `{other}` \
                         (allowed: strategy_id, since, status, limit)"
                    )));
                }
            }
        }
    }

    // Validate `since` as RFC3339 — silent failure would mask filter bugs.
    if let Some(s) = &since {
        if chrono::DateTime::parse_from_rfc3339(s).is_err() {
            return Err(invalid_params(format!(
                "execution://list: `since` must be RFC3339 / ISO8601 (e.g. `2026-05-14T00:00:00Z`), got `{s}`"
            )));
        }
    }

    // Map status label → RunStatus / journal_outcome. `noop` doesn't map to a
    // RunStatus variant — see `RunListFilter::journal_outcome` doc.
    let (status_run, journal_outcome) = match status_label.as_deref() {
        None => (None, None),
        Some("succeeded") => (Some(RunStatus::Succeeded), None),
        Some("failed") => (Some(RunStatus::Failed), None),
        Some("noop") => (None, Some("noop".to_string())),
        Some(other) => {
            return Err(invalid_params(format!(
                "execution://list: `status` must be one of `succeeded` | `failed` | `noop`, got `{other}`"
            )));
        }
    };

    // Parse + cap limit. Out-of-range u64 ⇒ invalid_params; >cap ⇒ silent clamp.
    let limit = match limit_raw {
        None => LIST_RUNS_DEFAULT_LIMIT,
        Some(raw) => raw.parse::<u64>().map_err(|e| {
            invalid_params(format!(
                "execution://list: `limit` must be a non-negative integer, got `{raw}`: {e}"
            ))
        })?,
    };
    if limit == 0 {
        return Err(invalid_params(
            "execution://list: `limit` must be ≥ 1".to_string(),
        ));
    }
    let limit = limit.min(LIST_RUNS_LIMIT_CAP);

    Ok(ParsedExecutionListQuery {
        strategy_id,
        since,
        status_label,
        status_run,
        journal_outcome,
        limit,
    })
}

/// Minimal percent-decoder for query-string values. Accepts `%XX` (case-
/// insensitive hex); rejects malformed escapes with a descriptive error.
/// We intentionally do NOT decode `+` to space — query strings are
/// path-style, not form-style, in the MCP URI grammar.
fn percent_decode(input: &str) -> Result<String, String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            if i + 2 >= bytes.len() {
                return Err(format!("truncated `%` escape at byte {i}"));
            }
            let hi = char::from(bytes[i + 1])
                .to_digit(16)
                .ok_or_else(|| format!("non-hex digit in `%` escape at byte {}", i + 1))?;
            let lo = char::from(bytes[i + 2])
                .to_digit(16)
                .ok_or_else(|| format!("non-hex digit in `%` escape at byte {}", i + 2))?;
            out.push(((hi << 4) | lo) as u8);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|e| format!("not valid UTF-8 after percent-decode: {e}"))
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
        "docs://strategy-bundle" => Some(DOC_STRATEGY_BUNDLE),
        _ => None,
    }
}

// ─────────── v1.4 Track B — strategy://list ───────────
//
// One resource read answers "what is running?" without forcing the agent to
// fan out to per-strategy fetches. Each summary embeds:
//   - id, name, description, tags, created_at
//   - trigger_kinds: the kinds of triggers attached (one per trigger row)
//   - last_fire_at: max(fired_at) across triggers (None when nothing fired)
//   - last_24h: { runs, succeeded, failed, actions } rolled up across runs
//   - has_bundle: True when records or view are present
//   - view_uri: the strategy://{id}/view follow-up
//
// Filters: `status=active|deleted|all` (default `active`), `tag=<name>`,
// `summary=true|false`. `summary=false` returns the bare summary fields.

#[derive(Default)]
struct ParsedStrategyListQuery {
    status: StrategyListStatus,
    tag: Option<String>,
    summary: bool,
}

#[derive(Default, Clone, Copy)]
enum StrategyListStatus {
    #[default]
    Active,
    Deleted,
    All,
}

fn parse_strategy_list_query(qs: &str) -> Result<ParsedStrategyListQuery, McpError> {
    let mut parsed = ParsedStrategyListQuery {
        status: StrategyListStatus::Active,
        tag: None,
        summary: true,
    };
    if qs.is_empty() {
        return Ok(parsed);
    }
    for pair in qs.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        let v = percent_decode(v).map_err(|e| {
            invalid_params(format!(
                "strategy://list: malformed percent-encoding in `{k}`: {e}"
            ))
        })?;
        match k {
            "status" => match v.as_str() {
                "active" => parsed.status = StrategyListStatus::Active,
                "deleted" => parsed.status = StrategyListStatus::Deleted,
                "all" => parsed.status = StrategyListStatus::All,
                other => {
                    return Err(invalid_params(format!(
                        "strategy://list: `status` must be one of `active` | `deleted` | `all`, got `{other}`"
                    )));
                }
            },
            "tag" => {
                if v.is_empty() {
                    return Err(invalid_params(
                        "strategy://list: `tag` value is empty",
                    ));
                }
                parsed.tag = Some(v);
            }
            "summary" => match v.as_str() {
                "true" | "1" | "" => parsed.summary = true,
                "false" | "0" => parsed.summary = false,
                other => {
                    return Err(invalid_params(format!(
                        "strategy://list: `summary` must be `true` | `false`, got `{other}`"
                    )));
                }
            },
            other => {
                return Err(invalid_params(format!(
                    "strategy://list: unknown query parameter `{other}` \
                     (allowed: status, tag, summary)"
                )));
            }
        }
    }
    Ok(parsed)
}

async fn read_strategy_list(
    uri: String,
    query: String,
    state: Arc<tokio::sync::Mutex<StateStore>>,
) -> Result<ReadResourceResult, McpError> {
    let parsed = parse_strategy_list_query(&query)?;

    // Pull strategies first. We always pull all (active + deleted) and filter
    // in Rust — the state layer's `list_strategies(true|false)` partitions
    // active from include-deleted; we filter further by `status` here.
    let summaries = {
        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            let store = state.blocking_lock();
            store.list_strategies(true)
        })
        .await
        .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
        .map_err(map_state_error)?
    };

    // status filter
    let filtered: Vec<_> = summaries
        .into_iter()
        .filter(|s| match parsed.status {
            StrategyListStatus::Active => s.deleted_at.is_none(),
            StrategyListStatus::Deleted => s.deleted_at.is_some(),
            StrategyListStatus::All => true,
        })
        .filter(|s| match &parsed.tag {
            None => true,
            Some(tag) => s
                .tags
                .as_ref()
                .is_some_and(|tags| tags.iter().any(|t| t == tag)),
        })
        .collect();

    // Build per-strategy summaries. The `last_24h` rollup queries the runs
    // table per strategy — for v1 we just call `list_runs` with a 24h since
    // bound + that strategy_id. We hold the StateStore mutex once for the
    // whole pass so there's no lock thrash.
    let ids: Vec<String> = filtered.iter().map(|s| s.id.clone()).collect();
    let state_for_lookup = state.clone();
    type StrategyAux = (
        // trigger_kinds
        Vec<String>,
        // last_fire_at
        Option<String>,
        // last_24h: (runs, succeeded, failed, actions)
        (u64, u64, u64, u64),
    );
    let aux: std::collections::HashMap<String, StrategyAux> =
        tokio::task::spawn_blocking(move || -> Result<_, StateError> {
            let store = state_for_lookup.blocking_lock();
            let since_24h = chrono::Utc::now() - chrono::Duration::hours(24);
            let since_24h_rfc =
                since_24h.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            let mut out: std::collections::HashMap<String, StrategyAux> =
                std::collections::HashMap::new();
            for sid in &ids {
                let triggers = store.list_triggers(Some(&TriggerListFilter {
                    strategy_id: Some(sid.clone()),
                    ..Default::default()
                }))?;
                let kinds: Vec<String> = triggers
                    .iter()
                    .map(|t| t.kind.as_wire().to_string())
                    .collect();
                let last_fire_at: Option<String> = triggers
                    .iter()
                    .filter_map(|t| t.last_fired_at.clone())
                    .max();

                // last_24h via list_runs filter.
                let recent = store.list_runs(&RunListFilter {
                    strategy_id: Some(sid.clone()),
                    since: Some(since_24h_rfc.clone()),
                    status: None,
                    journal_outcome: None,
                    limit: Some(LIST_RUNS_LIMIT_CAP),
                })?;
                let mut runs: u64 = 0;
                let mut succeeded: u64 = 0;
                let mut failed: u64 = 0;
                let mut actions: u64 = 0;
                for r in &recent {
                    runs += 1;
                    match r.status {
                        RunStatus::Succeeded => succeeded += 1,
                        RunStatus::Failed
                        | RunStatus::SimulationDenied
                        | RunStatus::PolicyDenied
                        | RunStatus::Canceled => failed += 1,
                        _ => {}
                    }
                    actions += u64::try_from(r.action_count).unwrap_or(0);
                }
                out.insert(sid.clone(), (kinds, last_fire_at, (runs, succeeded, failed, actions)));
            }
            Ok(out)
        })
        .await
        .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
        .map_err(map_state_error)?;

    let mut strategy_jsons: Vec<serde_json::Value> = Vec::with_capacity(filtered.len());
    let mut any_runs_in_24h = false;
    for s in &filtered {
        let entry = aux.get(&s.id).cloned().unwrap_or_default();
        let (kinds, last_fire_at, (runs, succeeded, failed, actions)) = entry;
        if runs > 0 {
            any_runs_in_24h = true;
        }
        if parsed.summary {
            strategy_jsons.push(json!({
                "id": s.id,
                "name": s.name,
                "description": s.description,
                "tags": s.tags,
                "created_at": s.created_at,
                "deleted_at": s.deleted_at,
                "trigger_kinds": kinds,
                "last_fire_at": last_fire_at,
                "last_24h": {
                    "runs": runs,
                    "succeeded": succeeded,
                    "failed": failed,
                    "actions": actions,
                },
                "has_bundle": s.has_bundle,
                "view_uri": format!("strategy://{id}/view", id = s.id),
            }));
        } else {
            strategy_jsons.push(json!({
                "id": s.id,
                "name": s.name,
                "description": s.description,
                "tags": s.tags,
                "created_at": s.created_at,
                "deleted_at": s.deleted_at,
                "has_bundle": s.has_bundle,
            }));
        }
    }

    // Honesty contract on the rollup: when no runs landed in 24h, declare
    // partial confidence so the agent doesn't infer "nothing is running" vs
    // "nothing ran recently".
    let body = if parsed.summary && !filtered.is_empty() && !any_runs_in_24h {
        json!({
            "strategies": strategy_jsons,
            "count": filtered.len(),
            "confidence": "partial",
            "reason": "no runs in the last 24h",
        })
    } else {
        json!({
            "strategies": strategy_jsons,
            "count": filtered.len(),
        })
    };
    let body_text = serde_json::to_string(&body)
        .map_err(|e| storage_error(format!("serialize strategy://list: {e}")))?;
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(body_text, uri).with_mime_type("application/json"),
    ]))
}

async fn read_strategy_by_name(
    uri: String,
    name: String,
    state: Arc<tokio::sync::Mutex<StateStore>>,
) -> Result<ReadResourceResult, McpError> {
    if name.is_empty() {
        return Err(McpError::resource_not_found(
            format!("malformed name in uri: {uri}"),
            Some(json!({
                "uri": uri,
                "code": "malformed_name",
                "hint": "name must be a non-empty URI segment",
            })),
        ));
    }
    let name_for_lookup = name.clone();
    let row = tokio::task::spawn_blocking(move || {
        let store = state.blocking_lock();
        store.get_strategy_by_name(&name_for_lookup)
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
    .map_err(map_state_error)?;
    match row {
        None => Err(McpError::resource_not_found(
            format!("strategy with name `{name}` not found"),
            Some(json!({
                "uri": uri,
                "code": "not_found",
                "hint": "list active strategies via strategy://list?status=active",
            })),
        )),
        Some(s) => {
            let resp = strategy_to_get_response(s);
            let body = serde_json::to_string(&resp)
                .map_err(|e| storage_error(format!("serialize strategy: {e}")))?;
            Ok(ReadResourceResult::new(vec![
                ResourceContents::text(body, uri).with_mime_type("application/json"),
            ]))
        }
    }
}

// ─────────── v1.4 Track B — trigger://list ───────────

fn parse_trigger_list_query(
    qs: &str,
) -> Result<TriggerListFilter, McpError> {
    let mut filter = TriggerListFilter::default();
    if qs.is_empty() {
        return Ok(filter);
    }
    for pair in qs.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        let v = percent_decode(v).map_err(|e| {
            invalid_params(format!(
                "trigger://list: malformed percent-encoding in `{k}`: {e}"
            ))
        })?;
        match k {
            "strategy_id" => filter.strategy_id = Some(v),
            "kind" => {
                let parsed = TriggerKind::from_wire(&v).ok_or_else(|| {
                    invalid_params(format!(
                        "trigger://list: `kind` must be one of `manual|interval|log|mempool|block|webhook`, got `{v}`"
                    ))
                })?;
                filter.kind = Some(parsed);
            }
            "enabled" => match v.as_str() {
                "true" | "1" => filter.enabled = Some(true),
                "false" | "0" => filter.enabled = Some(false),
                other => {
                    return Err(invalid_params(format!(
                        "trigger://list: `enabled` must be `true` | `false`, got `{other}`"
                    )));
                }
            },
            other => {
                return Err(invalid_params(format!(
                    "trigger://list: unknown query parameter `{other}` \
                     (allowed: strategy_id, kind, enabled)"
                )));
            }
        }
    }
    Ok(filter)
}

async fn read_trigger_list(
    uri: String,
    query: String,
    state: Arc<tokio::sync::Mutex<StateStore>>,
) -> Result<ReadResourceResult, McpError> {
    let filter = parse_trigger_list_query(&query)?;
    let summaries = tokio::task::spawn_blocking(move || {
        let store = state.blocking_lock();
        store.list_triggers(Some(&filter))
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
    .map_err(map_state_error)?;
    let count = summaries.len();
    let body = json!({
        "triggers": summaries,
        "count": count,
    });
    let body_text = serde_json::to_string(&body)
        .map_err(|e| storage_error(format!("serialize trigger://list: {e}")))?;
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(body_text, uri).with_mime_type("application/json"),
    ]))
}

// ─────────── v1.4 Track B — policy://current ───────────

async fn read_policy_current(
    uri: String,
    policy: Arc<RwLock<Option<LoadedPolicy>>>,
) -> Result<ReadResourceResult, McpError> {
    let guard = policy.read().await;
    let body = match &*guard {
        Some(loaded) => {
            let policy_json = serde_json::to_value(loaded).map_err(|e| {
                tracing::warn!(detail = %e, "policy://current: failed to serialize LoadedPolicy");
                storage_error(format!("policy://current serialize: {e}"))
            })?;
            json!({
                "loaded": true,
                "policy": policy_json,
                "confidence": "full",
            })
        }
        None => json!({
            "loaded": false,
            "reason": "policy not loaded (set [policy].path in config or fix policy.toml; see tracing logs)",
            "confidence": "missing",
            "remediation": "create .local/policy.toml — see docs://policy-model",
        }),
    };
    let body_text = serde_json::to_string(&body)
        .map_err(|e| storage_error(format!("policy://current encode: {e}")))?;
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(body_text, uri).with_mime_type("application/json"),
    ]))
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
        assert!(static_doc_for("docs://strategy-bundle").is_some());
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
async fn read_strategy_view(
    uri: String,
    id: String,
    state: Arc<tokio::sync::Mutex<StateStore>>,
) -> Result<ReadResourceResult, McpError> {
    if id.len() != 64 || !id.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)) {
        return Err(McpError::resource_not_found(
            format!("malformed strategy id in uri: {uri}"),
            Some(json!({ "uri": uri, "code": "malformed_id" })),
        ));
    }

    // 1. Load strategy + records snapshot in one blocking pass.
    let id_for_blocking = id.clone();
    let lookup = tokio::task::spawn_blocking(move || -> Result<_, StateError> {
        let store = state.blocking_lock();
        let s = store.get_strategy_by_id(&id_for_blocking)?;
        let records = store.list_strategy_records(&id_for_blocking, None, 500)?;
        Ok((s, records))
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
    .map_err(map_state_error)?;
    let (strategy, records_rows) = lookup;
    let Some(strategy) = strategy else {
        return Err(McpError::resource_not_found(
            format!("strategy {uri} not found"),
            Some(json!({ "uri": uri, "code": "not_found" })),
        ));
    };

    // 2. Fallback when no view source is registered.
    let view_source = match strategy.view_source.as_deref() {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => {
            let body = json!({
                "data": serde_json::Value::Null,
                "confidence": "missing",
                "reason": "strategy has no `view` function — register with bundle to enable",
                "remediation": "supply `view` (and ideally `records`) in strategy_register so this URI returns a strategy-defined snapshot",
            });
            let txt = serde_json::to_string(&body)
                .map_err(|e| storage_error(format!("serialize view fallback: {e}")))?;
            return Ok(ReadResourceResult::new(vec![
                ResourceContents::text(txt, uri).with_mime_type("application/json"),
            ]));
        }
    };

    // 3. Build the `records` argument: { recordName: { each, count, latest,
    //    sum(field) → value, since(ts) → array } }. We pre-compute sums for
    //    every numeric-castable field so the JS view function can call
    //    `records.supply.sum_amount` directly (host-computed), and we ship
    //    the raw rows under `each` for filtering / per-entry reads. Sum-on-
    //    demand is implemented as a JS lambda that closes over the rows.
    let records_arg = aggregate_records_for_view(&records_rows);

    // 4. Wrap the user's `(ctx, records) => any` so the existing Sandbox
    //    (which evals `(SOURCE)(__ctx)`) can call it. We inject the
    //    aggregated records as a JSON literal — safe because the aggregates
    //    are pure JSON values, not arbitrary JS.
    let records_json_lit = serde_json::to_string(&records_arg)
        .map_err(|e| storage_error(format!("serialize view records arg: {e}")))?;
    let wrapped_source = format!(
        "(ctx) => {{\n\
           const __records = {records};\n\
           const __view = ({user});\n\
           if (typeof __view !== 'function') throw new Error('view source must evaluate to a function (ctx, records) => any');\n\
           return __view(ctx, __records);\n\
         }}",
        records = records_json_lit,
        user = view_source
    );

    // 5. Run inside the same JS sandbox the existing `evm_view` tool uses.
    //    No provider here = the view function can still inspect args but RPC
    //    helpers will surface a typed JS error; that's fine — the v1.4
    //    contract for view is "ctx.evm.* mostly works" and the agent can
    //    diagnose via the wrapping `data` field.
    use strategy_js::{CtxHost, Sandbox};

    struct ViewHostInner {
        sid: String,
        name: String,
        logs: Vec<String>,
    }
    impl CtxHost for ViewHostInner {
        fn strategy_id(&self) -> &str { &self.sid }
        fn strategy_name(&self) -> &str { &self.name }
        fn run_id(&self) -> &str { "view" }
        fn now_millis(&self) -> i64 { 0 }
        fn append_log(&mut self, m: String) { self.logs.push(m); }
    }

    let sid_owned = strategy.id.clone();
    let name_owned = strategy.name.clone();
    let exec_result: Result<(serde_json::Value, Vec<String>), strategy_js::RuntimeError> =
        tokio::task::spawn_blocking(move || {
            let mut host = ViewHostInner {
                sid: sid_owned,
                name: name_owned,
                logs: Vec::new(),
            };
            let v = Sandbox::execute(&wrapped_source, &mut host)?;
            Ok((v, host.logs))
        })
        .await
        .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?;

    match exec_result {
        Ok((data, logs)) => {
            let body = json!({
                "data": data,
                "confidence": "full",
                "logs": logs,
            });
            let txt = serde_json::to_string(&body)
                .map_err(|e| storage_error(format!("serialize view ok: {e}")))?;
            Ok(ReadResourceResult::new(vec![
                ResourceContents::text(txt, uri).with_mime_type("application/json"),
            ]))
        }
        Err(e) => {
            // Honesty: report partial / unavailable rather than a 500. Agents
            // can inspect `reason` and act (re-register, fix view source, etc.).
            let body = json!({
                "data": serde_json::Value::Null,
                "confidence": "partial",
                "reason": format!("view function failed: {e}"),
                "remediation": "inspect `strategy://{id}` for the view source and try `evm_view` with a minimal repro",
            });
            let txt = serde_json::to_string(&body)
                .map_err(|err| storage_error(format!("serialize view err wrap: {err}")))?;
            Ok(ReadResourceResult::new(vec![
                ResourceContents::text(txt, uri).with_mime_type("application/json"),
            ]))
        }
    }
}

/// Build the `records` argument the JS view function sees. The DESIGN spec
/// asks for `{ sum(field), count, latest, since(ts), each }`. For v1 we
/// implement these by emitting a host-computed JSON snapshot per record name:
///
/// ```json
/// {
///   "supply": {
///     "count": 3,
///     "latest": { ... },        // newest captured row
///     "each":   [ ... ],        // all rows, newest-first
///     "sums":   { "amount": "12345", ... }
///   }
/// }
/// ```
///
/// Numeric sums are evaluated host-side over every JSON field whose values are
/// all decimal-string or JSON-number convertible to u128. Non-numeric fields
/// are simply omitted from `sums`. The view function reads the precomputed
/// value with `records.supply.sums.amount` (numbers as decimal strings to
/// preserve uint256 range).
fn aggregate_records_for_view(
    rows: &[executor_state::RecordCaptureEntry],
) -> serde_json::Value {
    use std::collections::BTreeMap;
    let mut by_name: BTreeMap<&str, Vec<&executor_state::RecordCaptureEntry>> = BTreeMap::new();
    for r in rows {
        by_name.entry(r.record_name.as_str()).or_default().push(r);
    }

    let mut out = serde_json::Map::new();
    for (name, rows) in by_name {
        // Rows are already newest-first from `list_strategy_records`; preserve.
        let parsed: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| serde_json::from_str(&r.payload_json).unwrap_or(serde_json::Value::Null))
            .collect();
        let latest = parsed.first().cloned().unwrap_or(serde_json::Value::Null);

        // Compute sums: walk every field key seen across rows. A field is
        // "summable" iff every observed value is numeric-string or JSON number.
        let mut field_values: BTreeMap<String, Vec<&serde_json::Value>> = BTreeMap::new();
        for p in &parsed {
            if let Some(obj) = p.as_object() {
                for (k, v) in obj {
                    field_values.entry(k.clone()).or_default().push(v);
                }
            }
        }
        let mut sums = serde_json::Map::new();
        for (k, vals) in field_values {
            if let Some(sum) = sum_decimal_strings(&vals) {
                sums.insert(k, serde_json::Value::String(sum));
            }
        }

        let mut entry = serde_json::Map::new();
        entry.insert(
            "count".to_string(),
            serde_json::Value::Number(serde_json::Number::from(rows.len())),
        );
        entry.insert("latest".to_string(), latest);
        entry.insert("each".to_string(), serde_json::Value::Array(parsed));
        entry.insert("sums".to_string(), serde_json::Value::Object(sums));
        out.insert(name.to_string(), serde_json::Value::Object(entry));
    }
    serde_json::Value::Object(out)
}

/// Sum a homogeneous slice of JSON values as u128 decimals. Returns `None`
/// if any value isn't decimal-castable (so the field is skipped from
/// `sums`). Overflow → `None` (the agent can still walk `each` manually).
fn sum_decimal_strings(vals: &[&serde_json::Value]) -> Option<String> {
    let mut acc: u128 = 0;
    let mut any = false;
    for v in vals {
        any = true;
        match v {
            serde_json::Value::String(s) => {
                let n = s.parse::<u128>().ok()?;
                acc = acc.checked_add(n)?;
            }
            serde_json::Value::Number(n) => {
                let v = n.as_u64()?;
                acc = acc.checked_add(v as u128)?;
            }
            _ => return None,
        }
    }
    if any { Some(acc.to_string()) } else { None }
}

// ─────────── v1.4 Track A4 — strategy://{id}/records ───────────

async fn read_strategy_records(
    uri: String,
    id: String,
    query: String,
    state: Arc<tokio::sync::Mutex<StateStore>>,
) -> Result<ReadResourceResult, McpError> {
    if id.len() != 64 || !id.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)) {
        return Err(McpError::resource_not_found(
            format!("malformed strategy id in uri: {uri}"),
            Some(json!({ "uri": uri, "code": "malformed_id" })),
        ));
    }

    // Parse `since` filter.
    let mut since: Option<String> = None;
    let mut limit: u64 = 500;
    if !query.is_empty() {
        for pair in query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let decoded = percent_decode(v).map_err(|e| {
                invalid_params(format!(
                    "strategy://{{id}}/records: malformed percent-encoding in `{k}`: {e}"
                ))
            })?;
            match k {
                "since" => since = Some(decoded),
                "limit" => {
                    limit = decoded.parse::<u64>().map_err(|e| {
                        invalid_params(format!(
                            "strategy://{{id}}/records: `limit` must be a non-negative integer: {e}"
                        ))
                    })?;
                    if limit == 0 {
                        return Err(invalid_params(
                            "strategy://{id}/records: `limit` must be ≥ 1".to_string(),
                        ));
                    }
                }
                other => {
                    return Err(invalid_params(format!(
                        "strategy://{{id}}/records: unknown query parameter `{other}` (allowed: since, limit)"
                    )));
                }
            }
        }
    }
    if let Some(s) = &since {
        if chrono::DateTime::parse_from_rfc3339(s).is_err() {
            return Err(invalid_params(format!(
                "strategy://{{id}}/records: `since` must be RFC3339 / ISO8601, got `{s}`"
            )));
        }
    }
    let capped = limit.min(500);

    let id_for_blocking = id.clone();
    let since_for_blocking = since.clone();
    let rows = tokio::task::spawn_blocking(move || -> Result<_, StateError> {
        let store = state.blocking_lock();
        store.list_strategy_records(&id_for_blocking, since_for_blocking.as_deref(), capped)
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))?
    .map_err(map_state_error)?;

    let rows_json: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let payload: serde_json::Value =
                serde_json::from_str(&r.payload_json).unwrap_or(serde_json::Value::Null);
            json!({
                "id": r.id,
                "run_id": r.run_id,
                "strategy_id": r.strategy_id,
                "record_name": r.record_name,
                "captured_at": r.captured_at,
                "payload": payload,
            })
        })
        .collect();
    let count = rows_json.len();
    let body = json!({
        "records": rows_json,
        "count": count,
        "filters_applied": {
            "since": since,
            "limit": capped,
        },
    });
    let txt = serde_json::to_string(&body)
        .map_err(|e| storage_error(format!("serialize records: {e}")))?;
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(txt, uri).with_mime_type("application/json"),
    ]))
}

