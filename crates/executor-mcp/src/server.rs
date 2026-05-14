//! `ExecutorServer` — rmcp 1.5 `ServerHandler` owning
//! `state: Arc<tokio::sync::Mutex<StateStore>>`.
//!
//! Phase 2 change: the Phase 1 no-arg `ExecutorServer::new()` and `Default`
//! impl are REMOVED. Opening a SQLite file can fail, so the constructor is
//! `new(&StateConfig) -> anyhow::Result<Self>`. `main.rs` is updated in this
//! same plan; in-tree integration tests adopt `spawn_server_with_state`.
//!
//! Async access: `StateStore` owns a bare `rusqlite::Connection` (Sync only
//! within a single thread). All DB calls go through
//! `tokio::task::spawn_blocking` + `state.blocking_lock()` (RESEARCH Pattern 2).
//! The tokio mutex is **never** held across an `await` (Pitfall 4).

use std::sync::Arc;

use anyhow::Result;
use executor_evm::{DynProvider, EvmConfig, EvmError};
use executor_policy::LoadedPolicy;
use executor_state::StateStore;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::router::{prompt::PromptRouter, tool::ToolRouter},
    model::{
        GetPromptRequestParams, GetPromptResult, ListPromptsResult, ListResourceTemplatesResult,
        ListResourcesResult, PaginatedRequestParams, ReadResourceRequestParams,
        ReadResourceResult, ServerCapabilities, ServerInfo,
    },
    prompt_handler,
    service::RequestContext,
    tool_handler,
};
use tokio::sync::{Mutex, RwLock};

use crate::{
    config::{Config, StateConfig},
    policy_boot::resolve_boot_policy,
    resources,
};

/// Self-documenting `instructions` payload returned in `initialize`. Read once
/// by the agent on connect — covers the mental model, the strategy/trigger/
/// policy surface, the `ctx` API, and pointers to the in-band prompts and
/// resources that go deeper.
const INSTRUCTIONS: &str = r#"# Onchain Strategy MCP

A local runtime that executes JS strategies onchain. You author short
JavaScript functions describing intent; the runtime simulates, signs with a
local burner, broadcasts, journals every decision, and (for multi-step plans)
auto-bundles into one atomic EIP-7702 transaction. Everything is local — keys
in the OS keychain, state in SQLite, no remote services.

When the user describes intent, WRITE THE STRATEGY YOURSELF and register it.
Do not ask the user for code. Strategies are short, declarative, and review
under 20 lines for most cases.

If you do not know where to start, call the `getting_started` prompt.
Before authoring a non-trivial strategy, read `docs://strategy-bundle` — it
covers the bundle shape (execute + records + view), the records capture DSL,
and the `$assets` convention used by `strategy://{id}/view` and the portfolio
aggregate to surface user positions.

## Three core concepts

1. **Strategy** — a JS function returning `Action[] | "noop"`. Registered via
   `strategy_register`, identified by a 64-char hex id, executed via
   `strategy_run`. Source is plain JS; the entry point is the top-level
   expression (no `module.exports`, no semicolons at EOF).

2. **Trigger** — *when* a strategy fires. Registered via `trigger_register`,
   attached to a strategy id. Without a trigger you must invoke `strategy_run`
   manually. Trigger kinds:

   | kind     | fires when                                              |
   |----------|----------------------------------------------------------|
   | manual   | An agent or user calls `strategy_run` directly           |
   | interval | Every N ms (cron-style)                                  |
   | log      | A confirmed log matches address + topic(s) filter        |
   | mempool  | A pending tx matches predicate on watched WSS node       |

3. **Policy** — a deny-by-default DSL gating signing. Loaded once from
   `.local/policy.toml`. Restricts allowed chains, contracts, function
   selectors, native value caps, and per-token ERC20 spend caps. Anything
   outside the policy is refused before broadcast. Inspect via `policy_get`.
   See `docs://policy-model` for the schema.

## Action shapes

A strategy returns an array of actions. The two supported builders:

```js
ctx.actions.contractCall({
  address: "0x...",       // target contract
  abi: [...],             // full ABI fragment (must include the fn)
  function: "supply",
  args: [token, amount, onBehalfOf, referralCode],
  value: "0",             // optional native value, decimal string wei
});

ctx.actions.erc20Approve({
  token:   "0x...",       // ERC20 contract
  spender: "0x...",       // who is being approved
  amount:  "1000000",     // decimal string in token base units
});
```

Returning `[approve, contractCall]` (or any multi-action array) triggers
automatic EIP-7702 bundling — both actions land in one tx via the BatchExec
delegate, or neither does. No manual batching call needed. See
`docs://eip-7702` for the deterministic CREATE2 address and the
`executor-mcp deploy-delegate` flow.

Return `"noop"` (or `null`) when the strategy decides no action is warranted
on this tick. Noop runs still journal source reads.

## `ctx.evm` read API

All reads are synchronous from the strategy's perspective (the JS sandbox
blocks the host thread on the call):

- `ctx.evm.nativeBalance(address, blockTag?)` → wei as decimal string
- `ctx.evm.erc20Balance(token, address, blockTag?)` → base units as string
- `ctx.evm.readContract({ address, abi, function, args, blockTag? })` →
  decoded return value (full ABI required; the runtime selects by name)
- `ctx.evm.code(address, blockTag?)` → hex string of deployed bytecode
- `ctx.evm.receipt(txHash)` → JSON receipt

`blockTag` accepts `"latest" | "pending" | "earliest" | "<block_number>"` —
historical reads work against archive RPCs. There is no async/await; do not
use `await` inside a strategy.

## Tool surface

- **Strategies:** `strategy_register`, `strategy_run`, `strategy_list`,
  `strategy_get`, `strategy_delete`
- **Triggers:** `trigger_register`, `trigger_list`, `trigger_get`,
  `trigger_set_enabled`, `trigger_delete`, `trigger_events`
- **Execution:** `execution_get` (receipt-backed report keyed by run id)
- **Policy:** `policy_set` (DESTRUCTIVE). Replaces the active policy
  revision; returns a JSON Patch diff + impact preview before the agent
  confirms. Read the current policy via `policy://current`; browse
  history via `policy://history`. Policy storage lives in SQLite — the
  v1.4 `.local/policy.toml` is imported once on first boot then ignored.
- **EVM reads:** `evm_receipt`, `evm_view` — `evm_view` runs ad-hoc JS in
  the same sandbox strategies use (`ctx.evm.nativeBalance` /
  `ctx.evm.erc20Balance` / `ctx.evm.readContract` / `ctx.evm.code`), so
  one-shot balance / code / contract-read lookups go through it

## Destructive ops (P5 — explicit consent)

Four tools mutate real state and are flagged with a literal `[DESTRUCTIVE]`
prefix at the start of their `description` field (visible via `tools/list`):

- `strategy_run` — signs and broadcasts real onchain transactions
- `strategy_delete` — soft-deletes a strategy row (recoverable but stops firing)
- `trigger_delete` — hard-deletes a trigger and its event history
- `policy_set` — replaces the active policy revision (v1.5 Track 1A)

Clients SHOULD parse the description and require explicit user consent before
invoking any tool tagged this way. The marker is a stable string contract:
match `^\[DESTRUCTIVE\]` against `tool.description`. (rmcp 1.5 does not expose
arbitrary JSON-Schema extensions through `#[tool(...)]`, so the description
prefix is the portable carrier — see `tools.rs` "Destructive ops" doc note.)

## Safe rehearsal

`strategy_register` accepts `dry_run: true` (default false). When set, the
server validates input and computes the would-be content-addressed id
(`sha256(source + records + view)`) WITHOUT inserting a row. The response
shape is `{ dry_run: true, would_be_strategy_id, name, has_bundle }`. Use it
to preview the id (and surface bundle/non-bundle status) before committing.

## Resources

- `strategy://{id}`, `execution://{run_id}`, `journal://{run_id}` — real JSON
- `trigger://{id}`, `trigger-events://{id}` — trigger row + last 100 events
- `examples://strategies` (list) and `examples://strategies/{name}` — embedded
  reference strategies (eth-funnel, yield-snapshot, erc20-approve,
  generic-counter-call)
- `examples://contracts/{name}` — embedded reference contracts (BatchExec)
- `docs://strategy-bundle` — bundle authoring guide (execute + records +
  view + `$assets` convention). Read before authoring non-trivial strategies.
- `policy://current`, `policy://history` — active policy + revision history
- `docs://policy-model`, `docs://eip-7702`, `docs://trigger-model` — concise
  prose docs

## Prompts to load when stuck

- `getting_started` — orient a fresh session end-to-end
- `trigger_patterns` — pick the right trigger kind
- `example_strategies` — menu of embedded examples to adapt
- `common_pitfalls` — short list of mistakes the runtime forgives badly
- `write_evm_strategy`, `review_evm_strategy` — guided authoring/review

## Error codes (JSON-RPC `data.kind` where applicable)

`-32011` strategy_deleted · `-32014` not_found · `-32015` name_conflict ·
`-32016` storage_error · `-32017` strategy_runtime_error
(kind ∈ timeout|oom|stack_overflow|exception|policy_not_loaded) ·
`-32018` strategy_invalid_output · `-32010` unimplemented ·
`-32602` invalid_params

## Operating defaults

- Burner address is the policy `signer`; use it as `simulation_from` when
  reading state-dependent prices. Other addresses simulate as the zero
  address and may revert.
- ETH transfers TO a 7702-delegated EOA need a BatchExec `receive()`. If a
  `selfdestruct`/native send to the burner reverts, that's why.
- The 7702 delegate ships at deterministic CREATE2 address
  `0x821fd81668823A3c5a65E95CeD5F050Ee54a4f53`. Run
  `npx onchain-strategy-mcp deploy-delegate` once per chain if
  `ctx.evm.code` (via `evm_view`) there is empty.
"#;

#[derive(Clone)]
pub struct ExecutorServer {
    pub(crate) tool_router: ToolRouter<Self>,
    pub(crate) prompt_router: PromptRouter<Self>,
    pub(crate) state: Arc<Mutex<StateStore>>,
    /// Phase 4 D-04: typed EVM config built from the `[evm]` section.
    /// Default values are used when no config file is supplied.
    pub(crate) evm_config: EvmConfig,
    /// Phase 4 D-04: lazy `Arc<DynProvider>`. Constructed on first
    /// `ctx.evm.*` call via [`ExecutorServer::evm_provider`]. Server boot
    /// is independent of devnet liveness.
    pub(crate) evm_provider: Arc<tokio::sync::OnceCell<Arc<DynProvider>>>,
    /// Phase 5 D-17: cached chain_id. Lazy-initialised via `chain_id()` on
    /// first call. `tokio::sync::OnceCell` does NOT memoize errors —
    /// transport failures retry on next call (operator may bring devnet up).
    pub(crate) chain_id_cell: Arc<tokio::sync::OnceCell<u64>>,
    /// Phase 5 Plan 05-03 / D-15: policy field. Loaded once at boot via
    /// [`Config::policy_config`]. Failure to load (missing file, bad TOML,
    /// bad address) leaves this field as `Arc<RwLock<None>>` and the orchestrator
    /// (Plan 05-04) returns -32017 `policy_not_loaded` on every `strategy_run`
    /// invocation until a valid policy is provided. `RwLock` (not `Mutex`)
    /// because future `policy_update` (v2) will swap the value while
    /// `strategy_run` reads concurrently.
    pub(crate) policy: Arc<RwLock<Option<LoadedPolicy>>>,
    /// v1.1 spike: optional EIP-7702 delegate. When `Some`, multi-action runs
    /// are bundled into one tx via `BatchExec.executeBatch`.
    ///
    /// v1.3: defaults to [`executor_signer::predicted_delegate_address`] when
    /// `[aa].delegate` is unset. The runtime checks `eth_getCode` at first
    /// batch attempt (memoized in [`Self::aa_delegate_verified`]) to fail
    /// fast with a structured error if the contract isn't deployed yet.
    pub(crate) aa_delegate: Option<alloy_primitives::Address>,
    /// v1.3: memoized result of the first `eth_getCode(aa_delegate)` check
    /// per server lifetime. `true` ⇒ batching is safe; `false` ⇒ delegate
    /// is missing and the agent must run `executor-mcp deploy-delegate`.
    /// `OnceCell` does not memoize errors, so transient RPC failures retry.
    pub(crate) aa_delegate_verified: Arc<tokio::sync::OnceCell<bool>>,
    /// v1.2 Stream E (mempool worker): shared WSS endpoint for
    /// `kind = mempool` triggers. `None` → mempool workers are skipped
    /// (warn-logged) at spawn time. Loaded from `[trigger].mempool_wss_url`.
    pub(crate) mempool_wss_url: Option<String>,
    /// v1.2 Trigger Core (Stream D): worker pool table — one `JoinHandle`
    /// per active background worker, keyed by trigger id.
    pub(crate) trigger_pool: Arc<Mutex<crate::triggers::pool::WorkerPool>>,
    /// v1.2 Trigger Core (Stream D): MPSC sender shared with each spawned
    /// worker and with the MCP `strategy_run` tool when it synthesizes a
    /// manual event. Backpressure is per-worker `try_send`.
    pub(crate) trigger_events_tx:
        tokio::sync::mpsc::Sender<crate::triggers::event::TriggerEvent>,
    /// v1.7 (`ctx.price.usd`): shared USD price cache. Single source of
    /// truth — used by `strategy_run` (RuntimeContext), `strategy://{id}/view`
    /// (ViewHostInner), and the idle balance walker (`web_portfolio`).
    pub price_cache: Arc<executor_evm::PriceCache>,
}

impl ExecutorServer {
    /// Phase 1-2 constructor — preserved for callers that don't supply a
    /// full [`Config`]. EVM config defaults to `EvmConfig::default()`
    /// (Phase 4 D-04).
    pub fn new(state_cfg: &StateConfig) -> Result<Self> {
        Self::new_with_config(state_cfg, &EvmConfig::default())
    }

    /// Phase 4 constructor variant — accepts a typed [`EvmConfig`] in
    /// addition to the storage path. The provider itself is NOT built here:
    /// it lazy-initialises on first `ctx.evm.*` call.
    ///
    /// Phase 5 Plan 05-03: policy field initialises to `None`. Use
    /// [`ExecutorServer::new_with_full_config`] to wire policy at boot from
    /// a parsed [`Config`].
    pub fn new_with_config(state_cfg: &StateConfig, evm_config: &EvmConfig) -> Result<Self> {
        let store = StateStore::open(std::path::Path::new(&state_cfg.path))
            .map_err(|e| anyhow::anyhow!("opening state store at {}: {e}", state_cfg.path))?;
        // v1.2 Trigger Core: channel + empty pool are created eagerly so the
        // synchronous (non-Arc) constructors stay usable. `from_config` is the
        // only entry point that actually wires the dispatcher task + spawns
        // pre-existing enabled triggers.
        let (trigger_events_tx, _rx) = tokio::sync::mpsc::channel(1024);
        Ok(Self {
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
            state: Arc::new(Mutex::new(store)),
            evm_config: evm_config.clone(),
            evm_provider: Arc::new(tokio::sync::OnceCell::new()),
            chain_id_cell: Arc::new(tokio::sync::OnceCell::new()),
            policy: Arc::new(RwLock::new(None)),
            aa_delegate: None,
            aa_delegate_verified: Arc::new(tokio::sync::OnceCell::new()),
            mempool_wss_url: None,
            trigger_pool: Arc::new(Mutex::new(crate::triggers::pool::WorkerPool::new())),
            trigger_events_tx,
            price_cache: Arc::new(executor_evm::PriceCache::new()),
        })
    }

    /// Phase 5 Plan 05-03: full-config constructor that ALSO loads the policy
    /// file (D-15 fail-closed). Boot proceeds even when the policy load fails
    /// — the `policy` field stays `None` and `strategy_run` (Plan 05-04)
    /// returns -32017 `policy_not_loaded`.
    pub fn new_with_full_config(
        state_cfg: &StateConfig,
        evm_config: &EvmConfig,
        full_cfg: &Config,
    ) -> Result<Self> {
        let mut srv = Self::new_with_config(state_cfg, evm_config)?;
        // v1.5 Track 1A: policy now lives in the DB. Boot resolution order:
        //   1. If `policies` table has an active row → load THAT (the TOML
        //      file is ignored on subsequent boots).
        //   2. Else, if `[policy].path` is configured AND the file exists,
        //      do a one-shot import → INSERT as the first revision with
        //      rationale "initial import from .local/policy.toml" and log a
        //      warning suggesting the operator delete / gitignore the TOML.
        //   3. Else, boot with `policy = None` (D-15 fail-closed).
        //
        // Malformed-TOML / address-parse failures during the import path do
        // NOT panic — we log `tracing::error!` and proceed with `None`. The
        // operator's recovery action is to call `policy_set` with a
        // corrected JSON body.
        let loaded: Option<LoadedPolicy> =
            resolve_boot_policy(&srv.state, full_cfg.policy.path.as_deref())?;
        srv.policy = Arc::new(RwLock::new(loaded));
        // v1.1 spike: optional [aa].delegate. Parsed via lenient EIP-55;
        // errors are logged but never block boot.
        // v1.2 Stream E: shared mempool WSS endpoint. Stored as-is; workers
        // validate it on connect (transient errors → reconnect with backoff).
        srv.mempool_wss_url = full_cfg.trigger.mempool_wss_url.clone();
        // v1.3: resolution order —
        //   1. `[aa].delegate` explicit override wins.
        //   2. Else, fall back to the CREATE2-predicted BatchExec address
        //      (`executor_signer::predicted_delegate_address`). The runtime
        //      verifies code-at-address lazily on first 7702 batch attempt
        //      (see `tools::execute_approved_actions`).
        match full_cfg.aa.delegate.as_deref() {
            Some(raw) => match raw.parse::<alloy_primitives::Address>() {
                Ok(addr) => {
                    tracing::info!(delegate = %addr, source = "config", "aa delegate loaded — multi-action runs will bundle via EIP-7702");
                    srv.aa_delegate = Some(addr);
                }
                Err(e) => {
                    let fallback = executor_signer::predicted_delegate_address();
                    tracing::error!(
                        raw = %raw,
                        error = %e,
                        fallback = %fallback,
                        "aa.delegate parse failed — falling back to CREATE2-predicted address",
                    );
                    srv.aa_delegate = Some(fallback);
                }
            },
            None => {
                let predicted = executor_signer::predicted_delegate_address();
                tracing::info!(
                    delegate = %predicted,
                    source = "create2_predicted",
                    "aa delegate auto-resolved via CREATE2 — run `executor-mcp deploy-delegate` once per chain to deploy",
                );
                srv.aa_delegate = Some(predicted);
            }
        }
        Ok(srv)
    }

    /// Convenience: build from a full [`Config`] (Phase 4 entry point + Phase
    /// 5 policy load).
    ///
    /// v1.2 Trigger Core (Stream D): returns `Arc<Self>` because the trigger
    /// dispatcher holds a `Weak<ExecutorServer>` to call back into the
    /// strategy-run pipeline. The bare `new_*` constructors still return
    /// `Self` for the test suite, which doesn't need the daemon.
    pub fn from_config(cfg: &Config) -> Result<Arc<Self>> {
        let evm_config = cfg
            .evm_config()
            .map_err(|e| anyhow::anyhow!("parsing [evm] config: {}", e.detail_for_log()))?;
        let mut server = Self::new_with_full_config(&cfg.state, &evm_config, cfg)?;
        // Replace the placeholder channel with a real one whose receiver feeds
        // the dispatcher we're about to spawn.
        let (tx, rx) = tokio::sync::mpsc::channel(1024);
        server.trigger_events_tx = tx.clone();
        let arc = Arc::new(server);
        // Spawn dispatcher with Weak so a dropped server lets the dispatcher
        // exit cleanly.
        let dispatcher = crate::triggers::dispatcher::Dispatcher {
            state: arc.state.clone(),
            server: Arc::downgrade(&arc),
        };
        tokio::spawn(dispatcher.run(rx));
        // Load enabled triggers and spawn workers for each. Storage errors
        // are logged but non-fatal — boot proceeds.
        let filter = executor_core::schema::trigger::TriggerListFilter {
            kind: None,
            enabled: Some(true),
            strategy_id: None,
        };
        // Use try_lock — `arc` is brand new and no other task can hold these
        // mutexes yet. Avoids `blocking_lock` which panics inside a tokio
        // runtime.
        let triggers = {
            let store = arc
                .state
                .try_lock()
                .expect("fresh state mutex not contended at boot");
            store.list_triggers(Some(&filter))
        };
        match triggers {
            Ok(list) => {
                let mut pool = arc
                    .trigger_pool
                    .try_lock()
                    .expect("fresh trigger_pool mutex not contended at boot");
                // list_triggers returns TriggerSummary; reload full Trigger
                // rows via get_trigger so the pool sees config_json/predicate.
                let store = arc
                    .state
                    .try_lock()
                    .expect("fresh state mutex not contended at boot");
                for summary in &list {
                    match store.get_trigger(&summary.id) {
                        Ok(Some(trigger)) => {
                            if let Err(e) =
                                pool.spawn(&trigger, tx.clone(), &arc.mempool_wss_url)
                            {
                                tracing::warn!(
                                    trigger_id = %trigger.id,
                                    error = %e,
                                    "failed to spawn worker at boot",
                                );
                            }
                        }
                        Ok(None) => {
                            tracing::warn!(
                                trigger_id = %summary.id,
                                "trigger summary present but row vanished",
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                trigger_id = %summary.id,
                                error = %e,
                                "failed to load trigger row at boot",
                            );
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to list triggers at boot");
            }
        }
        Ok(arc)
    }

    /// Lazy-init the alloy provider. First call constructs; subsequent
    /// calls return the cached `Arc`. Errors propagate as `EvmError` so
    /// transport failures surface as `-32017` instead of crashing the
    /// server (Phase 4 D-04).
    /// Phase 5 D-17: cached chain_id accessor. First call queries the
    /// provider via `eth_chainId`; subsequent calls return the cached value.
    /// Errors do NOT poison the cell — `OnceCell::get_or_try_init` retries
    /// on each Err, so a transient transport failure doesn't permanently
    /// disable strategy_run after the operator brings the devnet up.
    pub async fn chain_id(&self) -> Result<u64, EvmError> {
        let cell = self.chain_id_cell.clone();
        let provider = self.evm_provider().await?;
        cell.get_or_try_init(|| async move { executor_evm::fetch_chain_id(&provider).await })
            .await
            .copied()
    }

    /// v1.6 Track 6A: clone the shared `StateStore` handle so the web UI
    /// task can serve read-only `/api/*` queries without re-opening the DB.
    /// Returns the same `Arc<Mutex<_>>` used by the MCP resource handlers,
    /// so a write through one path is immediately visible through the
    /// other.
    pub fn state_handle(&self) -> Arc<Mutex<StateStore>> {
        self.state.clone()
    }

    pub async fn evm_provider(&self) -> Result<Arc<DynProvider>, EvmError> {
        let cell = self.evm_provider.clone();
        let cfg = self.evm_config.clone();
        // Type-annotate the closure return so OnceCell can infer the
        // success type (provider builder returns Arc<DynProvider>).
        cell.get_or_try_init(|| async move {
            executor_evm::build_provider(&cfg)
        })
        .await
        .cloned()
    }
}

// NOTE: Phase 1's `Default for ExecutorServer` and no-arg `new()` are REMOVED.
// With Phase 2, `new` is fallible and requires a `StateConfig`; there is no
// sensible default. Every caller must pass one. Integration tests use
// `crates/executor-mcp/tests/common::spawn_server_with_state` which writes a
// throwaway config file before spawning the binary.

// Pitfall 6: `#[tool_handler]` and `#[prompt_handler]` MUST share one
// `impl ServerHandler` block so the macros can co-generate `list_tools` /
// `call_tool` / `list_prompts` / `get_prompt` alongside the hand-written
// resource methods below.
#[tool_handler(router = self.tool_router)]
#[prompt_handler(router = self.prompt_router)]
impl ServerHandler for ExecutorServer {
    fn get_info(&self) -> ServerInfo {
        let caps = ServerCapabilities::builder()
            .enable_tools()
            .enable_prompts()
            .enable_resources()
            .build();
        ServerInfo::new(caps).with_instructions(INSTRUCTIONS)
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        resources::list_resources_impl(request, ctx).await
    }

    async fn list_resource_templates(
        &self,
        request: Option<PaginatedRequestParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        resources::list_resource_templates_impl(request, ctx).await
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        // Phase 2: pass the Arc<Mutex<StateStore>> so `strategy://{id}` reads
        // the real row. v1.4 Track B: also pass the policy snapshot for
        // `policy://current`. v1.6 fixup: thread the EVM provider so
        // `strategy://{id}/view` can run a view function that reads onchain
        // state (`ctx.evm.*`). Provider acquisition is fallible (RPC); we
        // degrade to `None` so non-view resources never fail over an
        // unreachable RPC.
        let provider = self.evm_provider().await.ok();
        // v1.7 (`ctx.price.usd`): chain_id is best-effort — failure to
        // resolve degrades the helper to a JS `null`, not a 500.
        let chain_id = self.chain_id().await.ok();
        let evm = resources::ViewEvm {
            provider,
            evm_config: self.evm_config.clone(),
            price_cache: Some(self.price_cache.clone()),
            chain_id,
        };
        resources::read_resource_impl(request, ctx, self.state.clone(), evm).await
    }
}

// ─────────── Phase 5 Plan 05-03 / D-15 fail-closed boot tests ───────────

#[cfg(test)]
mod policy_boot_tests {
    use super::*;
    use crate::config::Config;

    /// Build a temp `[state]` config dir for tests; mirror the pattern used
    /// elsewhere in the suite. Returns (state_cfg, full_cfg).
    fn make_cfg(state_path: &str, policy_toml_path: Option<&str>) -> (StateConfig, Config) {
        let mut full = Config::default();
        full.state.path = state_path.to_string();
        if let Some(p) = policy_toml_path {
            full.policy.path = Some(p.to_string());
        }
        (full.state.clone(), full)
    }

    #[tokio::test]
    async fn executor_server_boots_when_policy_load_fails() {
        // D-15: load failure must NOT panic; field stays None.
        let tmp = tempfile::tempdir().expect("tmp");
        let state_path = tmp.path().join("state.db");
        let (state_cfg, full_cfg) = make_cfg(
            state_path.to_str().unwrap(),
            Some("/no/such/__missing_policy_definitely__.toml"),
        );
        let evm = full_cfg.evm_config().expect("evm config");
        let server = ExecutorServer::new_with_full_config(&state_cfg, &evm, &full_cfg)
            .expect("server boots even when policy load fails (D-15)");
        assert!(
            server.policy.read().await.is_none(),
            "policy field is None after fail-closed boot"
        );
    }

    #[tokio::test]
    async fn executor_server_boots_when_policy_path_absent() {
        // [policy].path absent → policy = None → fail-closed at run time.
        let tmp = tempfile::tempdir().expect("tmp");
        let state_path = tmp.path().join("state.db");
        let (state_cfg, full_cfg) = make_cfg(state_path.to_str().unwrap(), None);
        let evm = full_cfg.evm_config().expect("evm config");
        let server = ExecutorServer::new_with_full_config(&state_cfg, &evm, &full_cfg)
            .expect("server boots when policy not configured");
        assert!(server.policy.read().await.is_none());
    }

    #[tokio::test]
    async fn executor_server_boots_with_valid_policy() {
        // Path points at the Plan 05-03 Task 1 fixture.
        let tmp = tempfile::tempdir().expect("tmp");
        let state_path = tmp.path().join("state.db");
        let fixture = "../executor-policy/tests/fixtures/policy.permissive.toml";
        let (state_cfg, full_cfg) = make_cfg(state_path.to_str().unwrap(), Some(fixture));
        let evm = full_cfg.evm_config().expect("evm config");
        let server = ExecutorServer::new_with_full_config(&state_cfg, &evm, &full_cfg)
            .expect("server boots with valid policy");
        let guard = server.policy.read().await;
        assert!(guard.is_some(), "policy is loaded");
        let p = guard.as_ref().unwrap();
        assert!(p.chains_allow.contains(&31337));
    }

    #[tokio::test]
    async fn executor_server_boots_with_malformed_policy_fails_closed() {
        // bad_address fixture → ValidationError at load → policy = None.
        let tmp = tempfile::tempdir().expect("tmp");
        let state_path = tmp.path().join("state.db");
        let fixture = "../executor-policy/tests/fixtures/policy.bad_address.toml";
        let (state_cfg, full_cfg) = make_cfg(state_path.to_str().unwrap(), Some(fixture));
        let evm = full_cfg.evm_config().expect("evm config");
        let server = ExecutorServer::new_with_full_config(&state_cfg, &evm, &full_cfg)
            .expect("server boots even when policy validation fails (D-15)");
        assert!(
            server.policy.read().await.is_none(),
            "policy field is None after malformed-policy fail-closed boot"
        );
    }
}
