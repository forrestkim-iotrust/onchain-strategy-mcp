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
    resources,
};

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
        Ok(Self {
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
            state: Arc::new(Mutex::new(store)),
            evm_config: evm_config.clone(),
            evm_provider: Arc::new(tokio::sync::OnceCell::new()),
            chain_id_cell: Arc::new(tokio::sync::OnceCell::new()),
            policy: Arc::new(RwLock::new(None)),
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
        // D-15 fail-closed: log + record None, never panic.
        let loaded: Option<LoadedPolicy> = match full_cfg.policy_config() {
            Ok(Some(p)) => {
                tracing::info!(
                    chains = ?p.chains_allow,
                    raw_call_global = p.raw_call_allow_global,
                    "policy loaded",
                );
                Some(p)
            }
            Ok(None) => {
                tracing::warn!(
                    "[policy].path not configured — strategy_run will fail-closed with policy_not_loaded"
                );
                None
            }
            Err(e) => {
                tracing::error!(
                    detail = %e.detail_for_log(),
                    kind = %e.data_kind(),
                    "policy load failed — strategy_run will fail-closed with policy_not_loaded",
                );
                None
            }
        };
        srv.policy = Arc::new(RwLock::new(loaded));
        Ok(srv)
    }

    /// Convenience: build from a full [`Config`] (Phase 4 entry point + Phase
    /// 5 policy load).
    pub fn from_config(cfg: &Config) -> Result<Self> {
        let evm_config = cfg
            .evm_config()
            .map_err(|e| anyhow::anyhow!("parsing [evm] config: {}", e.detail_for_log()))?;
        Self::new_with_full_config(&cfg.state, &evm_config, cfg)
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
        ServerInfo::new(caps).with_instructions(
            "Onchain Strategy MCP — Phase 3 runtime surface. \
             Strategy tools (strategy_register/list/get/delete) persist to a local \
             SQLite database. `strategy_run` executes a registered strategy in a \
             sandboxed JS runtime (Action[] | \"noop\" return) with full journaling \
             (journal_source_reads / journal_actions / journal_logs). \
             Sandbox failures surface as -32011 (strategy_deleted), -32017 \
             (strategy_runtime_error with data.kind ∈ timeout|oom|stack_overflow|exception), \
             or -32018 (strategy_invalid_output). \
             Storage errors use -32014 (not_found), -32015 (name_conflict), \
             -32016 (storage_error); validation failures use -32602 (invalid_params). \
             Resource templates: `strategy://{strategy_id}`, `journal://{run_id}`, \
             and `execution://{run_id}` return real JSON. `execution_get` and \
             `execution://{run_id}` return receipt-backed execution reports. \
             Only `policy_update` still returns the structured `unimplemented` envelope \
             (code -32010, data.phase=5).",
        )
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
        // the real row.
        resources::read_resource_impl(request, ctx, self.state.clone()).await
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
