//! Synchronous rquickjs-backed JavaScript sandbox (Phase 3).
//!
//! [`Sandbox::execute`] constructs a fresh `rquickjs::Runtime + Context::base`
//! per call (no pooling — RESEARCH Q6), applies the D-03 wall-clock / heap /
//! stack budgets, evaluates the strategy under the D-05 Shape-B contract
//! (`(ctx) => "noop" | Action[]`), rejects promise returns (D-10), and
//! converts the return value into a `serde_json::Value` so the MCP layer
//! (Plan 03-03) can semantically validate it against the `Action` enum.

use crate::error::RuntimeError;
use crate::limits::{
    GC_THRESHOLD_BYTES, MAX_STACK_BYTES, MEMORY_LIMIT_BYTES, WALL_CLOCK_MS,
};
use rquickjs::context::intrinsic;
use rquickjs::convert::Coerced;
use rquickjs::function::Rest;
use rquickjs::{CatchResultExt, Context, Ctx, Function, Object, Runtime, Value};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

/// Host-side context the strategy sees as `ctx`. Phase 3 buffers `ctx.log`
/// calls in `append_log` (no DB IO inside JS execution — RESEARCH Pitfall 2);
/// Plan 03-02 swaps this trait's impl from [`CtxStub`] to `RuntimeContext`
/// which flushes the buffer to `journal_logs` after `execute` returns.
///
/// Phase 4 (D-15a / HR-01 carry-forward) extends the trait additively —
/// `provider`, `evm_config`, `record_evm_read` have default impls so
/// existing impls (e.g. `CtxStub`) keep compiling. The host bindings for
/// `ctx.evm.*` install AFTER `FORBIDDEN_GLOBALS_SCRUB` (HR-01 lock).
pub trait CtxHost {
    fn strategy_id(&self) -> &str;
    fn strategy_name(&self) -> &str;
    fn run_id(&self) -> &str;
    fn now_millis(&self) -> i64;
    fn append_log(&mut self, message: String);

    /// Phase 4 D-04: shared `Arc<DynProvider>` for `ctx.evm.*` calls.
    /// Default `None` keeps `CtxStub` and other test hosts working —
    /// strategies that try `ctx.evm.readContract` against a `None`-provider
    /// host receive a typed JS error.
    fn provider(&self) -> Option<&std::sync::Arc<executor_evm::DynProvider>> {
        None
    }

    /// Phase 4 D-04: per-call timeout + RPC URL config. The default value
    /// is referenced by host bindings only when `provider()` is `Some`, so
    /// no test host needs to override.
    fn evm_config(&self) -> &executor_evm::EvmConfig {
        // SAFETY: a `static` value with a `Default::default()` body needs
        // `LazyLock` for thread-safety; we use `OnceLock` from std.
        use std::sync::OnceLock;
        static DEFAULT: OnceLock<executor_evm::EvmConfig> = OnceLock::new();
        DEFAULT.get_or_init(executor_evm::EvmConfig::default)
    }

    /// Phase 4 D-13: buffer one `journal_source_reads`-bound record per
    /// `ctx.evm.*` call. Default no-op so test hosts that don't journal can
    /// still satisfy the trait. `RuntimeContext` overrides to push into a
    /// drain buffer flushed alongside log records.
    fn record_evm_read(&mut self, _target: String, _payload: serde_json::Value) {}

    /// v1.2 Trigger Core: optional trigger event payload. Strategies invoked
    /// by manual trigger / external MCP call get `None`. Strategies invoked
    /// by interval/block/log/mempool triggers get `Some(payload)`.
    fn event(&self) -> Option<&serde_json::Value> {
        None
    }
}

/// In-memory `CtxHost` implementation used by Phase-3 unit tests and as the
/// type Plan 03-02 will replace at the MCP boundary.
#[derive(Debug, Default)]
pub struct CtxStub {
    pub strategy_id: String,
    pub strategy_name: String,
    pub run_id: String,
    pub logs: Vec<String>,
    /// v1.2 Trigger Core: optional trigger event payload. Tests opt in by
    /// setting `Some(...)`; default `None` leaves `ctx.event === null`.
    pub event: Option<serde_json::Value>,
}

impl CtxHost for CtxStub {
    fn strategy_id(&self) -> &str {
        &self.strategy_id
    }
    fn strategy_name(&self) -> &str {
        &self.strategy_name
    }
    fn run_id(&self) -> &str {
        &self.run_id
    }
    fn now_millis(&self) -> i64 {
        // Stub clock — Plan 03-02's RuntimeContext uses chrono::Utc::now.
        // Tests can pre-populate this if determinism is needed.
        0
    }
    fn append_log(&mut self, message: String) {
        self.logs.push(message);
    }
    fn event(&self) -> Option<&serde_json::Value> {
        self.event.as_ref()
    }
}

/// Synchronous JavaScript sandbox. Construction is free (unit struct);
/// `execute` constructs a fresh rquickjs `Runtime + Context::base` per call.
pub struct Sandbox;

impl Sandbox {
    /// Evaluate a strategy under the D-03 budgets and the D-04 `ctx`
    /// surface. **Caller wraps in `tokio::task::spawn_blocking`** —
    /// rquickjs `Runtime` is `!Sync` without the `parallel` feature.
    ///
    /// Phase 3 wires only the Shape-B entry-point + D-11 deny-by-default
    /// intrinsic surface; the `_host` parameter is currently passed
    /// through unused (Plan 03-02 wires the real `ctx` host bindings).
    pub fn execute<H: CtxHost>(
        source: &str,
        host: &mut H,
    ) -> Result<serde_json::Value, RuntimeError> {
        // 1. Fresh runtime per call (RESEARCH Concurrency Plan / Pitfall 6).
        let rt = Runtime::new()
            .map_err(|e| RuntimeError::EngineInit(format!("Runtime::new: {e}")))?;
        rt.set_memory_limit(MEMORY_LIMIT_BYTES);
        rt.set_gc_threshold(GC_THRESHOLD_BYTES);
        rt.set_max_stack_size(MAX_STACK_BYTES);

        // 2. Wall-clock interrupt. Tracks deadline-hit so we can disambiguate
        //    Timeout from a generic Exception in the error path (Pitfall 14).
        let deadline = Instant::now() + Duration::from_millis(WALL_CLOCK_MS);
        let timed_out = Arc::new(AtomicBool::new(false));
        let timed_out_clone = timed_out.clone();
        rt.set_interrupt_handler(Some(Box::new(move || {
            if Instant::now() >= deadline {
                timed_out_clone.store(true, Ordering::SeqCst);
                true
            } else {
                false
            }
        })));

        // 3. D-11: build the context from `Context::base` semantics — i.e. the
        //    minimal intrinsic set EXCLUDING module/import/require/loader. We
        //    use `Context::builder().with::<intrinsic::All>()` because the
        //    bare `Context::base` call (no intrinsics beyond base objects)
        //    does NOT include the `Eval` intrinsic and rejects user code with
        //    "eval is not supported" — Phase-3 strategies must be evaluable.
        //    `intrinsic::All` enumerates Date / Eval / RegExp / JSON / Proxy /
        //    MapSet / TypedArrays / Promise / BigInt / Performance / WeakRef
        //    (rquickjs 0.11 `context/builder.rs:73-86`), and crucially does
        //    NOT include any module/import/require/loader intrinsic — those
        //    only arrive via `Context::full` (which uses `JS_NewContext`
        //    instead of `JS_NewContextRaw`). The D-11 invariant — no
        //    module/import/require — is preserved. Context::full is still
        //    forbidden.
        let ctx = Context::builder()
            .with::<intrinsic::All>()
            .build(&rt)
            .map_err(|e| RuntimeError::EngineInit(format!("Context::builder: {e}")))?;

        // 4. Evaluate inside Context::with — rquickjs::Value is `'js`-bound
        //    (Pitfall 5). All conversion to serde_json must happen here.
        //
        // Plan 03-02: install the real D-04 ctx surface (strategy / run /
        // now / log / actions.noop). Logs are buffered host-side via a
        // single-threaded `Rc<RefCell<Vec<String>>>` shared between the
        // `ctx.log` closure and the post-`ctx.with` drain pass — no DB IO
        // inside the JS callback (RESEARCH Pitfall 2).
        let log_buffer: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let log_buffer_for_drain = log_buffer.clone();

        // Phase 4 D-13 buffer for `ctx.evm.*` journal records, drained
        // alongside logs after `ctx.with` returns.
        let evm_reads: Rc<RefCell<Vec<crate::runtime::EvmReadRecord>>> =
            Rc::new(RefCell::new(Vec::new()));
        let evm_reads_for_drain = evm_reads.clone();

        // Snapshot host fields BEFORE `ctx.with` — closures must own their
        // captures (rquickjs `Function::new` requires `'js`, not `&'js mut H`).
        let strategy_id_owned = host.strategy_id().to_string();
        let strategy_name_owned = host.strategy_name().to_string();
        let run_id_owned = host.run_id().to_string();
        let now_value = host.now_millis() as f64;
        let provider_clone: Option<std::sync::Arc<executor_evm::DynProvider>> =
            host.provider().cloned();
        let evm_cfg_clone: executor_evm::EvmConfig = host.evm_config().clone();
        // v1.2 Trigger Core: snapshot the optional trigger event payload so
        // the closure below can install `ctx.event` (null when None).
        let event_snapshot: Option<serde_json::Value> = host.event().cloned();

        let result = ctx.with(|c| -> Result<serde_json::Value, RuntimeError> {
            // 4a. D-04 ctx surface — real injection (replaces the 03-01 stub).
            let ctx_obj = Object::new(c.clone())
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

            // strategy.id, strategy.name (read-only string fields).
            let strategy_obj = Object::new(c.clone())
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            strategy_obj
                .set("id", strategy_id_owned.as_str())
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            strategy_obj
                .set("name", strategy_name_owned.as_str())
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            ctx_obj
                .set("strategy", strategy_obj)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

            // run.id (read-only string field).
            let run_obj = Object::new(c.clone())
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            run_obj
                .set("id", run_id_owned.as_str())
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            ctx_obj
                .set("run", run_obj)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

            // ctx.now() — captured snapshot at injection time. Phase-3
            // determinism: agent-visible "now" is fixed for the run; Phase-4+
            // may revisit if intra-strategy clock progression is needed.
            let now_fn = Function::new(c.clone(), move || -> rquickjs::Result<f64> {
                Ok(now_value)
            })
            .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            ctx_obj
                .set("now", now_fn)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

            // ctx.log(...args) — JS-spec String() coercion via `Coerced<String>`,
            // single-space joined, appended to the host buffer. NO DB IO in
            // the callback (Pitfall 2).
            let buf = log_buffer.clone();
            let log_fn = Function::new(
                c.clone(),
                move |args: Rest<Coerced<String>>| -> rquickjs::Result<()> {
                    let parts: Vec<String> = args.0.into_iter().map(|c| c.0).collect();
                    buf.borrow_mut().push(parts.join(" "));
                    Ok(())
                },
            )
            .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            ctx_obj
                .set("log", log_fn)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

            // ctx.actions.noop() — returns the literal "noop".
            let actions_obj = Object::new(c.clone())
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            let noop_fn = Function::new(c.clone(), || -> rquickjs::Result<String> {
                Ok("noop".to_string())
            })
            .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            actions_obj
                .set("noop", noop_fn)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

            // Phase 4 D-08 / D-09 (CTX-05/06/07/08): action builders.
            // Each builder is a sync host function: validates inputs (address
            // shape, hex calldata, decimal amount, ABI parse + dry-run encode)
            // and returns a JS Object that round-trips through the
            // executor-mcp validate_strategy_output gate as the matching
            // Action variant.
            //
            // CRITICAL ORDERING (D-15a / HR-01): these closures are added to
            // `actions_obj` BEFORE the FORBIDDEN_GLOBALS_SCRUB eval below; the
            // scrub still runs BEFORE `c.globals().set("__ctx", ...)`, so a
            // future intrinsic colliding with `__ctx` cannot delete a binding
            // we already installed.
            macro_rules! install_builder {
                ($name:expr, $kind:expr) => {{
                    let kind: ActionBuilderKind = $kind;
                    let f = Function::new(
                        c.clone(),
                        make_action_builder_closure(kind),
                    )
                    .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
                    actions_obj
                        .set($name, f)
                        .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
                }};
            }
            install_builder!("contractCall", ActionBuilderKind::ContractCall);
            install_builder!("rawCall", ActionBuilderKind::RawCall);
            install_builder!("erc20Transfer", ActionBuilderKind::Erc20Transfer);
            install_builder!("erc20Approve", ActionBuilderKind::Erc20Approve);
            install_builder!("nativeTransfer", ActionBuilderKind::NativeTransfer);

            ctx_obj
                .set("actions", actions_obj)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

            // Phase 4 D-04 / D-13: ctx.evm sub-object. `readContract` is
            // installed regardless of whether `provider()` is `Some` —
            // strategies that invoke it without a configured provider get
            // a typed JS error (not a missing-namespace ReferenceError).
            // This sub-object is built BEFORE the FORBIDDEN_GLOBALS_SCRUB
            // line below (alongside the other ctx.* sub-objects); the
            // scrub still runs BEFORE `c.globals().set("__ctx", ...)` is
            // called, preserving HR-01 ordering for the install of `__ctx`
            // onto globalThis.
            let evm_obj = Object::new(c.clone())
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            let provider_for_fn = provider_clone.clone();
            let cfg_for_fn = evm_cfg_clone.clone();
            let evm_reads_for_fn = evm_reads.clone();
            // Bind the input/output lifetime via an annotated helper trait
            // signature: the closure must declare a single `'js` for both
            // the incoming Object and the returned Value.
            fn make_read_contract_closure(
                provider: Option<std::sync::Arc<executor_evm::DynProvider>>,
                cfg: executor_evm::EvmConfig,
                evm_reads: Rc<RefCell<Vec<crate::runtime::EvmReadRecord>>>,
            ) -> impl for<'js> Fn(rquickjs::Object<'js>) -> rquickjs::Result<rquickjs::Value<'js>>
                   + 'static {
                move |args: rquickjs::Object<'_>| {
                    read_contract_host_binding(
                        &args,
                        provider.as_ref(),
                        &cfg,
                        &evm_reads,
                    )
                }
            }
            let read_contract_fn = Function::new(
                c.clone(),
                make_read_contract_closure(
                    provider_for_fn,
                    cfg_for_fn,
                    evm_reads_for_fn,
                ),
            )
            .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            evm_obj
                .set("readContract", read_contract_fn)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

            // Phase 4 D-06 / D-07 / CTX-02 / CTX-03 / CTX-04: ERC20 + native
            // helper bindings. Structured forms live under
            // `ctx.evm.readErc20.*` and `ctx.evm.readNative.*`; the flat
            // aliases REQUIREMENTS demands (`erc20Balance`, `erc20Allowance`,
            // `nativeBalance`) sit alongside `readErc20`/`readNative` on
            // `ctx.evm`. All forms route to the SAME backing helper functions
            // in `executor_evm::{erc20, native}` — flat-alias and structured
            // calls with identical arguments produce identical results and
            // identical journal payloads (D-15 / threat T-04-02-01).
            //
            // Each helper records exactly one `journal_source_reads` row with
            // `kind="evm_read"`, `target="<lower_address>:<helper_function>"`,
            // and `payload_json.helper` set to the helper name (e.g.
            // `"balanceOf"`) — NOT the alias name. The structured-form name
            // is the canonical helper identity; aliases are name-only.
            let read_erc20 = Object::new(c.clone())
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

            macro_rules! install_erc20 {
                ($obj:expr, $name:expr, $kind:expr) => {{
                    let provider_local = provider_clone.clone();
                    let cfg_local = evm_cfg_clone.clone();
                    let buf_local = evm_reads.clone();
                    let kind: Erc20Helper = $kind;
                    let f = Function::new(
                        c.clone(),
                        make_erc20_closure(provider_local, cfg_local, buf_local, kind),
                    )
                    .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
                    $obj.set($name, f)
                        .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
                }};
            }
            install_erc20!(read_erc20, "balanceOf", Erc20Helper::BalanceOf);
            install_erc20!(read_erc20, "allowance", Erc20Helper::Allowance);
            install_erc20!(read_erc20, "decimals", Erc20Helper::Decimals);
            install_erc20!(read_erc20, "symbol", Erc20Helper::Symbol);
            install_erc20!(read_erc20, "name", Erc20Helper::Name);
            install_erc20!(read_erc20, "totalSupply", Erc20Helper::TotalSupply);
            evm_obj
                .set("readErc20", read_erc20)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

            // readNative — native_balance + native_block_number
            let read_native = Object::new(c.clone())
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            {
                let provider_local = provider_clone.clone();
                let cfg_local = evm_cfg_clone.clone();
                let buf_local = evm_reads.clone();
                let f = Function::new(
                    c.clone(),
                    make_native_balance_closure(provider_local, cfg_local, buf_local),
                )
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
                read_native
                    .set("balance", f)
                    .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            }
            {
                let provider_local = provider_clone.clone();
                let cfg_local = evm_cfg_clone.clone();
                let buf_local = evm_reads.clone();
                let f = Function::new(
                    c.clone(),
                    make_native_block_number_closure(provider_local, cfg_local, buf_local),
                )
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
                read_native
                    .set("blockNumber", f)
                    .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            }
            evm_obj
                .set("readNative", read_native)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

            // Flat aliases per REQUIREMENTS naming (CTX-02 / CTX-03 / CTX-04).
            // Each alias is a separate JS Function whose body invokes the SAME
            // Rust helper (executor_evm::erc20::erc20_{balance_of, allowance}
            // / executor_evm::native::native_balance) the structured form
            // does — identical results, identical journal payloads.
            install_erc20!(evm_obj, "erc20Balance", Erc20Helper::BalanceOf);
            install_erc20!(evm_obj, "erc20Allowance", Erc20Helper::Allowance);
            {
                let provider_local = provider_clone.clone();
                let cfg_local = evm_cfg_clone.clone();
                let buf_local = evm_reads.clone();
                let f = Function::new(
                    c.clone(),
                    make_native_balance_closure(provider_local, cfg_local, buf_local),
                )
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
                evm_obj
                    .set("nativeBalance", f)
                    .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            }

            ctx_obj
                .set("evm", evm_obj)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

            // Phase 4 D-10 / D-11 / CTX-09: ctx.units + ctx.address surfaces
            // (Plan 04-04). Pure host-side helpers — no provider, no journal,
            // no async. Built BEFORE FORBIDDEN_GLOBALS_SCRUB at the SAME site
            // as the other ctx.* sub-objects (HR-01 carry-forward — the scrub
            // still runs BEFORE `c.globals().set("__ctx", ...)`).
            let units_obj = Object::new(c.clone())
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            let parse_units_fn = Function::new(c.clone(), make_parse_units_closure())
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            units_obj
                .set("parseUnits", parse_units_fn)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            let format_units_fn = Function::new(c.clone(), make_format_units_closure())
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            units_obj
                .set("formatUnits", format_units_fn)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            ctx_obj
                .set("units", units_obj)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

            let address_obj = Object::new(c.clone())
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            let is_address_fn = Function::new(c.clone(), make_is_address_closure())
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            address_obj
                .set("isAddress", is_address_fn)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            let checksum_fn = Function::new(c.clone(), make_address_checksum_closure())
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            address_obj
                .set("checksum", checksum_fn)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            // ZERO_ADDRESS is a STRING constant — not a function. Reassigning
            // `ctx.address.zeroAddress` in strategy code only affects the
            // strategy's local view; host-side reads always go through the
            // Rust constant `executor_evm::ZERO_ADDRESS` (T-04-04-02).
            address_obj
                .set("zeroAddress", executor_evm::ZERO_ADDRESS)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
            ctx_obj
                .set("address", address_obj)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

            // v1.2 Trigger Core: install `ctx.event`. When the host carries
            // a trigger payload it's projected into a JS value (Object /
            // Array / primitive) via `json_to_qjs_value`; otherwise `null`.
            // Strategies invoked manually observe `ctx.event === null`;
            // strategies invoked by interval/block/log/mempool triggers
            // observe the payload object. Installed at the same site as the
            // other `ctx.*` sub-objects (BEFORE FORBIDDEN_GLOBALS_SCRUB and
            // the `__ctx` global set — HR-01 ordering preserved).
            let event_js_val: rquickjs::Value = match event_snapshot.as_ref() {
                Some(v) => json_to_qjs_value(&c, v).map_err(|detail| {
                    RuntimeError::EngineInit(format!("ctx.event install: {detail}"))
                })?,
                None => rquickjs::Value::new_null(c.clone()),
            };
            ctx_obj
                .set("event", event_js_val)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

            // 4a-prime. D-11 scrub MUST run BEFORE host bindings are installed.
            // The Promise intrinsic ships `queueMicrotask` on globalThis; remove
            // it so the forbidden-globals regression suite holds. The list below
            // mirrors D-11 and is defensive against future intrinsic additions:
            // we delete each name unconditionally — a `delete` of an
            // already-absent property is a no-op.
            //
            // Ordering rationale (HR-01): if a future intrinsic surfaced a name
            // that overlapped a host binding (e.g. a hypothetical `__ctx`
            // intrinsic), running the scrub AFTER `c.globals().set("__ctx", …)`
            // would silently delete the host binding. By scrubbing FIRST and
            // installing host bindings AFTER, the contract is robust against
            // rquickjs upgrades that add new intrinsics.
            c.eval::<(), _>(
                FORBIDDEN_GLOBALS_SCRUB.as_bytes().to_vec(),
            )
            .catch(&c)
            .map_err(|caught| caught_to_runtime_error(caught, &timed_out))?;

            // Host binding install — AFTER the D-11 scrub.
            c.globals()
                .set("__ctx", ctx_obj)
                .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

            // 4b. D-05 Shape B: source MUST evaluate to a function. We wrap
            //     in an IIFE that returns either the call result or the
            //     sentinel `__STRATEGY_NOT_FUNCTION__` so the failure mode
            //     is observable cleanly from Rust.
            let wrapped = format!(
                "(() => {{ const __fn = ({src}); \
                   if (typeof __fn !== 'function') return '__STRATEGY_NOT_FUNCTION__'; \
                   return __fn(__ctx); \
                 }})()",
                src = source
            );

            let value: Value = c
                .eval::<Value, _>(wrapped.into_bytes())
                .catch(&c)
                .map_err(|caught| caught_to_runtime_error(caught, &timed_out))?;

            // 4c. D-10: reject promise returns explicitly.
            if value.is_promise() {
                return Err(RuntimeError::InvalidOutput {
                    detail: "promise return values are not supported in v1; \
                             strategies must be synchronous"
                        .into(),
                });
            }

            // 4d. Convert to serde_json::Value INSIDE this closure.
            let json = qjs_value_to_json(&value)
                .map_err(|detail| RuntimeError::InvalidOutput { detail })?;

            // 4e. D-05 Shape-B sentinel.
            if matches!(&json, serde_json::Value::String(s) if s == "__STRATEGY_NOT_FUNCTION__")
            {
                return Err(RuntimeError::InvalidOutput {
                    detail: "strategy source must evaluate to a function `(ctx) => \"noop\" | Action[]` \
                             (D-05 Shape B); top-level expressions and \
                             named-function-on-globalThis are not accepted"
                        .into(),
                });
            }

            Ok(json)
        });

        // 5. Drain the host-side log buffer (filled by `ctx.log` callbacks
        //    inside the closure) into the host BEFORE we propagate any error.
        //    Even on a Timeout/Exception we still want to surface logs the
        //    strategy emitted up to the failure point. Borrow_mut() works
        //    regardless of whether the rquickjs runtime retained Rc clones.
        {
            let drained: Vec<String> = log_buffer_for_drain.borrow_mut().drain(..).collect();
            for msg in drained {
                host.append_log(msg);
            }
        }
        // Phase 4 D-13: drain the ctx.evm.* journal buffer into the host
        // (RuntimeContext::record_evm_read pushes into evm_reads, then
        // flush() writes them to journal_source_reads with kind="evm_read").
        {
            let drained: Vec<crate::runtime::EvmReadRecord> =
                evm_reads_for_drain.borrow_mut().drain(..).collect();
            for record in drained {
                host.record_evm_read(record.target, record.payload_json);
            }
        }

        // 6. Outside the closure: prefer `Timeout` over `Exception` if the
        //    interrupt handler raised the deadline flag (Pitfall 14).
        match result {
            Ok(v) => Ok(v),
            Err(RuntimeError::Exception(_)) if timed_out.load(Ordering::SeqCst) => {
                Err(RuntimeError::Timeout)
            }
            Err(e) => Err(e),
        }
    }

    /// v1.2 Trigger Core: evaluate a predicate JS function
    /// `(event) => bool` in an isolated sandbox under the same wall-clock /
    /// heap / stack budgets as [`Sandbox::execute`]. The predicate is pure:
    /// no `ctx`, no actions, no evm bindings — just the event payload.
    ///
    /// Returns `Ok(false)` defensively when:
    ///   - the predicate throws
    ///   - the predicate returns a non-boolean
    ///   - the evaluation hits timeout / OOM / stack overflow
    ///
    /// Returns `Err(RuntimeError::EngineInit(...))` only when the engine
    /// itself fails to initialize (extremely rare — host-level OOM).
    ///
    /// Caller wraps in `tokio::task::spawn_blocking` — rquickjs `Runtime`
    /// is `!Sync`.
    pub fn evaluate_predicate(
        source: &str,
        event: &serde_json::Value,
    ) -> Result<bool, RuntimeError> {
        let rt = Runtime::new()
            .map_err(|e| RuntimeError::EngineInit(format!("Runtime::new: {e}")))?;
        rt.set_memory_limit(MEMORY_LIMIT_BYTES);
        rt.set_gc_threshold(GC_THRESHOLD_BYTES);
        rt.set_max_stack_size(MAX_STACK_BYTES);

        let deadline = Instant::now() + Duration::from_millis(WALL_CLOCK_MS);
        let timed_out = Arc::new(AtomicBool::new(false));
        let timed_out_clone = timed_out.clone();
        rt.set_interrupt_handler(Some(Box::new(move || {
            if Instant::now() >= deadline {
                timed_out_clone.store(true, Ordering::SeqCst);
                true
            } else {
                false
            }
        })));

        let ctx = Context::builder()
            .with::<intrinsic::All>()
            .build(&rt)
            .map_err(|e| RuntimeError::EngineInit(format!("Context::builder: {e}")))?;

        let result: Result<bool, RuntimeError> =
            ctx.with(|c| -> Result<bool, RuntimeError> {
                // Install __event onto globalThis (a single binding — no ctx
                // surface, no actions, no evm; predicates are pure & fast).
                let event_val = json_to_qjs_value(&c, event).map_err(|detail| {
                    RuntimeError::EngineInit(format!("predicate event install: {detail}"))
                })?;
                c.globals()
                    .set("__event", event_val)
                    .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;

                // D-11 scrub for parity with execute() — predicates shouldn't
                // see console/fetch/setTimeout either.
                c.eval::<(), _>(FORBIDDEN_GLOBALS_SCRUB.as_bytes().to_vec())
                    .catch(&c)
                    .map_err(|caught| caught_to_runtime_error(caught, &timed_out))?;

                // Wrap so non-bool returns coerce to false via `=== true`.
                let wrapped = format!(
                    "(() => {{ const __fn = ({src}); const r = __fn(__event); return r === true; }})()",
                    src = source
                );
                let value: Coerced<bool> = c
                    .eval::<Coerced<bool>, _>(wrapped.into_bytes())
                    .catch(&c)
                    .map_err(|caught| caught_to_runtime_error(caught, &timed_out))?;
                Ok(value.0)
            });

        match result {
            Ok(b) => Ok(b),
            // Defensive: throws, timeouts, OOM, stack overflow, non-bool
            // returns all collapse to `Ok(false)`. Only engine-init errors
            // (which surface from outside `ctx.with`) propagate as Err.
            Err(_) => Ok(false),
        }
    }
}

/// JavaScript prelude that scrubs D-11 forbidden globals from `globalThis`.
/// QuickJS's `Promise` intrinsic exposes `queueMicrotask`; future intrinsic
/// versions may surface other names. We `delete` each unconditionally so
/// the deny-by-default contract is enforced even when an intrinsic leaks.
const FORBIDDEN_GLOBALS_SCRUB: &str = r#"
    (function() {
        const names = [
            "console", "fetch",
            "setTimeout", "setInterval", "setImmediate", "queueMicrotask",
            "XMLHttpRequest", "WebSocket",
            "process", "Worker",
            "child_process", "fs",
            "Deno",
        ];
        for (const n of names) {
            try { delete globalThis[n]; } catch (e) { /* ignore non-configurable */ }
        }
    })();
"#;

/// Convert a [`rquickjs::CaughtError`] into a typed [`RuntimeError`]. The
/// caller handles the deadline-hit override after `Context::with` returns.
fn caught_to_runtime_error(
    caught: rquickjs::CaughtError<'_>,
    timed_out: &Arc<AtomicBool>,
) -> RuntimeError {
    if timed_out.load(Ordering::SeqCst) {
        return RuntimeError::Timeout;
    }
    match caught {
        rquickjs::CaughtError::Exception(ex) => {
            let msg = ex
                .message()
                .unwrap_or_else(|| "<no exception message>".into());
            let classified = classify_message(&msg);
            classified.unwrap_or(RuntimeError::Exception(msg))
        }
        rquickjs::CaughtError::Value(v) => {
            // Best-effort string coercion for non-Error throws (`throw 42`).
            let msg = v
                .as_string()
                .and_then(|s| s.to_string().ok())
                .unwrap_or_else(|| format!("thrown {}", v.type_name()));
            classify_message(&msg).unwrap_or(RuntimeError::Exception(msg))
        }
        rquickjs::CaughtError::Error(e) => {
            let msg = e.to_string();
            classify_message(&msg).unwrap_or(RuntimeError::Exception(msg))
        }
    }
}

/// Convenience wrapper used at points where `Ctx::catch` must be invoked
/// against a bare `rquickjs::Error`.
fn classify_qjs_error(
    c: &Ctx<'_>,
    e: rquickjs::Error,
    timed_out: &Arc<AtomicBool>,
) -> RuntimeError {
    let caught = rquickjs::CaughtError::from_error(c, e);
    caught_to_runtime_error(caught, timed_out)
}

/// Heuristic message classifier — maps QuickJS exception text to typed
/// RuntimeError variants. Returns `None` when no heuristic matches; the
/// caller falls back to `Exception(msg)`. Heuristics are case-insensitive
/// substring matches against canonical QuickJS messages observed in 0.11.
fn classify_message(msg: &str) -> Option<RuntimeError> {
    let lower = msg.to_lowercase();
    if lower.contains("out of memory") || lower.contains("oom") {
        return Some(RuntimeError::Oom);
    }
    if lower.contains("stack overflow") || lower.contains("maximum call stack") {
        return Some(RuntimeError::StackOverflow);
    }
    if lower.contains("interrupted") {
        // The interrupt handler-raised exception surfaces as a generic
        // "interrupted" message. The caller's deadline-flag check will
        // already have converted this to Timeout in the common case;
        // emitting Timeout here too defends against the rare path where
        // the flag isn't set (e.g. a future API change).
        return Some(RuntimeError::Timeout);
    }
    // BR-01: re-classify EVM stable-prefix exceptions back into
    // `RuntimeError::Evm(_)` so the MCP boundary maps them onto the D-12
    // `data.kind ∈ {evm_rpc_error, evm_decode_error, evm_revert}` taxonomy
    // instead of a generic `data.kind = "exception"`. The host bindings
    // throw `EvmError::Display` (e.g. `"evm rpc error: timeout"`) verbatim;
    // this classifier matches each stable prefix and reconstructs the
    // typed variant. The runtime category for re-thrown Decode/Encode is
    // carried as a `Cow::Owned` string (BR-01-coupled fix in
    // `executor-evm::EvmError`).
    use executor_evm::EvmError;
    use std::borrow::Cow;
    // Trim the leading "Error: " prefix QuickJS prepends to thrown Error
    // messages — `caught_to_runtime_error` may pass either form. Also
    // peel off the builder-context prefix `"ctx.actions.<helper>: "` (and
    // any `args[i]:` host-binding prefix) so the EvmError stable string
    // is matched at the tail. We scan for the rightmost occurrence of an
    // `evm ` taxonomy prefix instead of requiring it at position 0.
    let body = msg.strip_prefix("Error: ").unwrap_or(msg);
    // Find a stable taxonomy prefix anywhere in the body and slice from there.
    let body = if let Some(idx) = body.find("evm rpc error: ")
        .or_else(|| body.find("evm decode error: "))
        .or_else(|| body.find("evm encode error: "))
        .or_else(|| body.find("evm revert: "))
        .or_else(|| body.find("evm provider config error"))
    {
        &body[idx..]
    } else {
        body
    };
    if body == "evm rpc error: timeout" {
        return Some(RuntimeError::Evm(EvmError::Timeout));
    }
    if body == "evm rpc error: transport" {
        return Some(RuntimeError::Evm(EvmError::Transport {
            detail_for_log: "<re-thrown from JS>".into(),
        }));
    }
    if body == "evm provider config error" {
        return Some(RuntimeError::Evm(EvmError::Config {
            detail_for_log: "<re-thrown from JS>".into(),
        }));
    }
    if let Some(reason) = body.strip_prefix("evm revert: ") {
        return Some(RuntimeError::Evm(EvmError::Revert {
            reason: reason.into(),
            detail_for_log: "<re-thrown from JS>".into(),
        }));
    }
    if let Some(category) = body.strip_prefix("evm decode error: ") {
        return Some(RuntimeError::Evm(EvmError::Decode {
            category: Cow::Owned(category.to_string()),
            detail_for_log: "<re-thrown from JS>".into(),
        }));
    }
    if let Some(category) = body.strip_prefix("evm encode error: ") {
        return Some(RuntimeError::Evm(EvmError::Encode {
            category: Cow::Owned(category.to_string()),
            detail_for_log: "<re-thrown from JS>".into(),
        }));
    }
    None
}

/// Phase 4 D-04 host binding for `ctx.evm.readContract`. Synchronous from
/// the JS side; performs the RPC by acquiring the current Tokio runtime
/// handle and calling `block_on(read_contract(...))`. The storage mutex
/// is NEVER acquired in this path (D-04 mutex discipline).
///
/// The closure throws a typed JS Error when:
/// - no `Arc<DynProvider>` is wired (host returned `None` from `provider()`),
/// - args are malformed (missing fields, wrong types, BigInt — D-03),
/// - the underlying `read_contract` returns `EvmError` (transport / decode
///   / revert / timeout). The thrown `Error.message` carries `EvmError::Display`
///   (wire-safe stable string), which the MCP boundary classifies via the
///   exception classifier (Phase 3 `classify_message`) — fallback to
///   `RuntimeError::Exception(stable_string)`. The wire taxonomy upgrade to
///   `evm_*` `data.kind` is the responsibility of `executor-mcp` mapping
///   (Plan 04-01 Task 2 `map_evm_error`); here we just emit the typed
///   exception text so QuickJS surfaces it correctly.
fn read_contract_host_binding<'js>(
    args: &Object<'js>,
    provider: Option<&std::sync::Arc<executor_evm::DynProvider>>,
    cfg: &executor_evm::EvmConfig,
    evm_reads: &Rc<RefCell<Vec<crate::runtime::EvmReadRecord>>>,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    use executor_evm::read::{BlockTag, ReadContractInput};
    let ctx = args.ctx().clone();

    let provider = match provider {
        Some(p) => p.clone(),
        None => {
            return Err(throw_js_error(
                &ctx,
                "ctx.evm.readContract not available: no provider configured",
            ));
        }
    };

    // Extract fields. We accept abi as JSON-string OR JS array (D-05).
    let address: rquickjs::Value = args.get("address")?;
    let address: String = address
        .as_string()
        .ok_or_else(|| throw_js_error(&ctx, "address must be a string"))?
        .to_string()?;

    let abi_value: rquickjs::Value = args.get("abi")?;
    let abi_json: String = if let Some(s) = abi_value.as_string() {
        s.to_string()?
    } else if abi_value.is_array() || abi_value.is_object() {
        // JS array / object → JSON.stringify on the host side via our walker.
        let json = qjs_value_to_json(&abi_value)
            .map_err(|e| throw_js_error(&ctx, &format!("abi: {e}")))?;
        serde_json::to_string(&json)
            .map_err(|e| throw_js_error(&ctx, &format!("abi serialize: {e}")))?
    } else {
        return Err(throw_js_error(
            &ctx,
            "abi must be a JSON string or an array of fragments",
        ));
    };

    let function_name: rquickjs::Value = args.get("function")?;
    let function_name: String = function_name
        .as_string()
        .ok_or_else(|| throw_js_error(&ctx, "function must be a string"))?
        .to_string()?;

    let args_value: rquickjs::Value = args.get("args")?;
    // WR-07: pre-walk args[] to detect BigInt at args[i] before
    // `qjs_value_to_json` reaches it. The walker emits a generic "BigInt not
    // supported in strategy RETURNS" message that's wrong for an INPUT path
    // and lacks the D-03 builder-style hint about ctx.units.parseUnits(...).
    if let Some(arr) = args_value.as_array() {
        for i in 0..arr.len() {
            let item: rquickjs::Value = arr
                .get::<rquickjs::Value>(i)
                .map_err(|e| throw_js_error(&ctx, &format!("args[{i}]: {e}")))?;
            if matches!(item.type_of(), rquickjs::Type::BigInt) {
                return Err(throw_js_error(
                    &ctx,
                    &format!(
                        "args[{i}] must be a decimal string, got BigInt — use \
                         ctx.units.parseUnits(...) to produce one"
                    ),
                ));
            }
        }
    }
    let json_args = qjs_value_to_json(&args_value)
        .map_err(|e| throw_js_error(&ctx, &format!("args: {e}")))?;
    let args_arr: Vec<serde_json::Value> = match json_args {
        serde_json::Value::Array(v) => v,
        // Missing args → empty list (helpful for niladic functions).
        serde_json::Value::Null => Vec::new(),
        other => {
            return Err(throw_js_error(
                &ctx,
                &format!("args must be an array, got {}", json_value_kind(&other)),
            ));
        }
    };

    let block_tag = match args.get::<_, rquickjs::Value>("blockTag") {
        Ok(v) if v.is_undefined() || v.is_null() => BlockTag::Latest,
        Ok(v) => parse_block_tag(&v).map_err(|e| throw_js_error(&ctx, &e))?,
        Err(_) => BlockTag::Latest,
    };

    let address_lower = address.to_lowercase();
    let target = format!("{address_lower}:{function_name}");

    let input = ReadContractInput {
        address: address.clone(),
        abi_json: abi_json.clone(),
        function: function_name.clone(),
        args: args_arr.clone(),
        block_tag,
    };

    // Concurrency Plan: we are inside `Sandbox::execute`, which the caller
    // wraps in `tokio::task::spawn_blocking`. The current thread therefore
    // has a Tokio runtime handle available via `Handle::try_current()`.
    // Acquire it WITHOUT holding any storage mutex (D-04 mutex discipline)
    // and `block_on` the async read_contract. If no runtime handle is
    // present (e.g. CtxStub-driven unit tests run from a synchronous
    // `#[test]` outside any tokio context), fall back to a transient
    // current-thread runtime.
    let result: Result<serde_json::Value, executor_evm::EvmError> =
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => handle.block_on(executor_evm::read_contract(provider.clone(), cfg, input)),
            Err(_) => {
                // No ambient runtime: spin up a transient single-threaded
                // runtime. This path is only reached from synchronous unit
                // tests that don't go through the MCP `strategy_run` handler.
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| {
                        throw_js_error(
                            &ctx,
                            &format!("evm rpc error: runtime build failed: {e}"),
                        )
                    })?;
                rt.block_on(executor_evm::read_contract(provider.clone(), cfg, input))
            }
        };

    let json = match result {
        Ok(v) => v,
        Err(e) => {
            // Wire-safe Display (D-12). Detail-for-log goes via tracing.
            let stable = e.to_string();
            tracing::warn!(
                detail = %e.detail_for_log(),
                kind = %e.data_kind(),
                "ctx.evm.readContract failed"
            );
            return Err(throw_js_error(&ctx, &stable));
        }
    };

    // Phase 4 D-13: journal the read (kind="evm_read").
    let resolved = resolve_block_number(&provider, cfg, block_tag);
    let mut payload = serde_json::json!({
        "helper": "readContract",
        "args": args_arr,
        "function": function_name,
        "address": address,
        "block_tag": block_tag_to_json(block_tag),
    });
    if let (Some(n), Some(obj)) = (resolved, payload.as_object_mut()) {
        obj.insert("block_number_resolved".into(), serde_json::Value::from(n));
    }
    evm_reads
        .borrow_mut()
        .push(crate::runtime::EvmReadRecord {
            target,
            payload_json: payload,
        });

    json_to_qjs_value(&ctx, &json).map_err(|e| throw_js_error(&ctx, &e))
}

/// Phase 4 D-06 helper kinds. Each variant maps 1:1 to a function in the
/// bundled `executor_evm::erc20` ABI. The helper name appears verbatim in
/// the journal payload (`payload.helper`) — the JS-side flat aliases
/// (`erc20Balance` / `erc20Allowance`) re-use these same kinds, so the
/// helper identity recorded in the journal is the structured-form name.
#[derive(Debug, Clone, Copy)]
enum Erc20Helper {
    BalanceOf,
    Allowance,
    Decimals,
    Symbol,
    Name,
    TotalSupply,
}

impl Erc20Helper {
    fn helper_name(self) -> &'static str {
        match self {
            Erc20Helper::BalanceOf => "balanceOf",
            Erc20Helper::Allowance => "allowance",
            Erc20Helper::Decimals => "decimals",
            Erc20Helper::Symbol => "symbol",
            Erc20Helper::Name => "name",
            Erc20Helper::TotalSupply => "totalSupply",
        }
    }
    /// Number of address-shaped positional args BEFORE the optional blockTag
    /// (token is always arg 0; allowance also takes owner+spender).
    fn arg_arity(self) -> usize {
        match self {
            Erc20Helper::BalanceOf => 2,    // token, account
            Erc20Helper::Allowance => 3,    // token, owner, spender
            Erc20Helper::Decimals => 1,     // token
            Erc20Helper::Symbol => 1,       // token
            Erc20Helper::Name => 1,         // token
            Erc20Helper::TotalSupply => 1,  // token
        }
    }
}

fn make_erc20_closure(
    provider: Option<std::sync::Arc<executor_evm::DynProvider>>,
    cfg: executor_evm::EvmConfig,
    evm_reads: Rc<RefCell<Vec<crate::runtime::EvmReadRecord>>>,
    kind: Erc20Helper,
) -> impl for<'js> Fn(rquickjs::function::Rest<rquickjs::Value<'js>>) -> rquickjs::Result<rquickjs::Value<'js>>
       + 'static {
    move |args: rquickjs::function::Rest<rquickjs::Value<'_>>| {
        erc20_host_binding(args.0, provider.as_ref(), &cfg, &evm_reads, kind)
    }
}

fn make_native_balance_closure(
    provider: Option<std::sync::Arc<executor_evm::DynProvider>>,
    cfg: executor_evm::EvmConfig,
    evm_reads: Rc<RefCell<Vec<crate::runtime::EvmReadRecord>>>,
) -> impl for<'js> Fn(rquickjs::function::Rest<rquickjs::Value<'js>>) -> rquickjs::Result<rquickjs::Value<'js>>
       + 'static {
    move |args: rquickjs::function::Rest<rquickjs::Value<'_>>| {
        native_balance_host_binding(args.0, provider.as_ref(), &cfg, &evm_reads)
    }
}

fn make_native_block_number_closure(
    provider: Option<std::sync::Arc<executor_evm::DynProvider>>,
    cfg: executor_evm::EvmConfig,
    evm_reads: Rc<RefCell<Vec<crate::runtime::EvmReadRecord>>>,
) -> impl for<'js> Fn(Ctx<'js>) -> rquickjs::Result<rquickjs::Value<'js>> + 'static {
    move |ctx: Ctx<'_>| {
        native_block_number_host_binding(ctx, provider.as_ref(), &cfg, &evm_reads)
    }
}

/// Phase 4 D-06 host binding for `ctx.evm.readErc20.*` and the flat aliases
/// (`erc20Balance` / `erc20Allowance`). Positional JS args:
///   - balanceOf(token, account, blockTag?)
///   - allowance(token, owner, spender, blockTag?)
///   - decimals(token, blockTag?)
///   - symbol(token, blockTag?)
///   - name(token, blockTag?)
///   - totalSupply(token, blockTag?)
///
/// `blockTag` defaults to `"latest"` when missing or `undefined` (NOTE-2 from
/// plan-checker: `flat_alias_default_blockTag_is_latest` test pins this).
fn erc20_host_binding<'js>(
    raw_args: Vec<rquickjs::Value<'js>>,
    provider: Option<&std::sync::Arc<executor_evm::DynProvider>>,
    cfg: &executor_evm::EvmConfig,
    evm_reads: &Rc<RefCell<Vec<crate::runtime::EvmReadRecord>>>,
    kind: Erc20Helper,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    use executor_evm::read::BlockTag;

    // Recover ctx via the FIRST argument (we always have at least the token
    // arg; the host binding rejects empty arg lists). When raw_args is
    // empty we have to construct a Ctx from a borrowed value — but rquickjs
    // requires a Ctx for throwing. We bail with a generic Error path: this
    // is structurally impossible for our binding because rquickjs's Function
    // fn-like glue ensures Rest<Value> is always given a context.
    let ctx = match raw_args.first() {
        Some(v) => v.ctx().clone(),
        None => return Err(rquickjs::Error::Exception),
    };

    let provider = match provider {
        Some(p) => p.clone(),
        None => {
            return Err(throw_js_error(
                &ctx,
                &format!(
                    "ctx.evm.readErc20.{} not available: no provider configured",
                    kind.helper_name()
                ),
            ));
        }
    };

    let arity = kind.arg_arity();
    if raw_args.len() < arity {
        return Err(throw_js_error(
            &ctx,
            &format!(
                "ctx.evm.readErc20.{} expects at least {arity} positional arg(s), got {}",
                kind.helper_name(),
                raw_args.len()
            ),
        ));
    }

    let mut addrs: Vec<String> = Vec::with_capacity(arity);
    for (i, v) in raw_args.iter().take(arity).enumerate() {
        let s = v
            .as_string()
            .ok_or_else(|| {
                throw_js_error(
                    &ctx,
                    &format!(
                        "ctx.evm.readErc20.{}: arg #{} must be a string (address)",
                        kind.helper_name(),
                        i
                    ),
                )
            })?
            .to_string()?;
        addrs.push(s);
    }

    // Optional blockTag. Tag arg index = arity (zero-indexed; e.g. balanceOf
    // takes (token, account) so blockTag is at index 2). Missing or
    // `undefined` → Latest (NOTE-2 default).
    let block_tag = if raw_args.len() > arity {
        let v = &raw_args[arity];
        if v.is_undefined() || v.is_null() {
            BlockTag::Latest
        } else {
            parse_block_tag(v).map_err(|e| throw_js_error(&ctx, &e))?
        }
    } else {
        BlockTag::Latest
    };

    let token = addrs[0].clone();
    let result: Result<serde_json::Value, executor_evm::EvmError> = {
        let provider = provider.clone();
        let cfg = cfg.clone();
        let block_tag_for_call = block_tag;
        let call_addrs = addrs.clone();
        let dispatch = async move {
            match kind {
                Erc20Helper::BalanceOf => {
                    executor_evm::erc20::erc20_balance_of(
                        provider,
                        &cfg,
                        &call_addrs[0],
                        &call_addrs[1],
                        block_tag_for_call,
                    )
                    .await
                }
                Erc20Helper::Allowance => {
                    executor_evm::erc20::erc20_allowance(
                        provider,
                        &cfg,
                        &call_addrs[0],
                        &call_addrs[1],
                        &call_addrs[2],
                        block_tag_for_call,
                    )
                    .await
                }
                Erc20Helper::Decimals => {
                    executor_evm::erc20::erc20_decimals(
                        provider,
                        &cfg,
                        &call_addrs[0],
                        block_tag_for_call,
                    )
                    .await
                }
                Erc20Helper::Symbol => {
                    executor_evm::erc20::erc20_symbol(
                        provider,
                        &cfg,
                        &call_addrs[0],
                        block_tag_for_call,
                    )
                    .await
                }
                Erc20Helper::Name => {
                    executor_evm::erc20::erc20_name(
                        provider,
                        &cfg,
                        &call_addrs[0],
                        block_tag_for_call,
                    )
                    .await
                }
                Erc20Helper::TotalSupply => {
                    executor_evm::erc20::erc20_total_supply(
                        provider,
                        &cfg,
                        &call_addrs[0],
                        block_tag_for_call,
                    )
                    .await
                }
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => handle.block_on(dispatch),
            Err(_) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| {
                        throw_js_error(
                            &ctx,
                            &format!("evm rpc error: runtime build failed: {e}"),
                        )
                    })?;
                rt.block_on(dispatch)
            }
        }
    };

    let json = match result {
        Ok(v) => v,
        Err(e) => {
            let stable = e.to_string();
            tracing::warn!(
                detail = %e.detail_for_log(),
                kind = %e.data_kind(),
                helper = kind.helper_name(),
                "ctx.evm.readErc20.* failed"
            );
            return Err(throw_js_error(&ctx, &stable));
        }
    };

    // Phase 4 D-13 journal record. Helper-specific args (allowance has 2
    // address args; the rest have 1).
    let token_lower = token.to_lowercase();
    let target = format!("{token_lower}:{}", kind.helper_name());
    let payload_args: Vec<serde_json::Value> = addrs
        .iter()
        .skip(1)
        .map(|s| serde_json::Value::String(s.clone()))
        .collect();
    let resolved = resolve_block_number(&provider, cfg, block_tag);
    let mut payload = serde_json::json!({
        "helper": kind.helper_name(),
        "args": payload_args,
        "address": token,
        "block_tag": block_tag_to_json(block_tag),
    });
    if let (Some(n), Some(obj)) = (resolved, payload.as_object_mut()) {
        obj.insert("block_number_resolved".into(), serde_json::Value::from(n));
    }
    evm_reads
        .borrow_mut()
        .push(crate::runtime::EvmReadRecord {
            target,
            payload_json: payload,
        });

    json_to_qjs_value(&ctx, &json).map_err(|e| throw_js_error(&ctx, &e))
}

/// Phase 4 D-07 host binding for `ctx.evm.readNative.balance` + the flat
/// alias `ctx.evm.nativeBalance`. Positional args:
///   - balance(account, blockTag?)
fn native_balance_host_binding<'js>(
    raw_args: Vec<rquickjs::Value<'js>>,
    provider: Option<&std::sync::Arc<executor_evm::DynProvider>>,
    cfg: &executor_evm::EvmConfig,
    evm_reads: &Rc<RefCell<Vec<crate::runtime::EvmReadRecord>>>,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    use executor_evm::read::BlockTag;

    let ctx = match raw_args.first() {
        Some(v) => v.ctx().clone(),
        None => return Err(rquickjs::Error::Exception),
    };

    let provider = match provider {
        Some(p) => p.clone(),
        None => {
            return Err(throw_js_error(
                &ctx,
                "ctx.evm.readNative.balance not available: no provider configured",
            ));
        }
    };

    if raw_args.is_empty() {
        return Err(throw_js_error(
            &ctx,
            "ctx.evm.readNative.balance expects at least 1 positional arg (account), got 0",
        ));
    }
    let account = raw_args[0]
        .as_string()
        .ok_or_else(|| throw_js_error(&ctx, "ctx.evm.readNative.balance: account must be a string"))?
        .to_string()?;

    let block_tag = if raw_args.len() > 1 {
        let v = &raw_args[1];
        if v.is_undefined() || v.is_null() {
            BlockTag::Latest
        } else {
            parse_block_tag(v).map_err(|e| throw_js_error(&ctx, &e))?
        }
    } else {
        BlockTag::Latest
    };

    let dispatch =
        executor_evm::native::native_balance(provider.clone(), cfg, &account, block_tag);
    let result: Result<serde_json::Value, executor_evm::EvmError> =
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => handle.block_on(dispatch),
            Err(_) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| {
                        throw_js_error(
                            &ctx,
                            &format!("evm rpc error: runtime build failed: {e}"),
                        )
                    })?;
                rt.block_on(dispatch)
            }
        };

    let json = match result {
        Ok(v) => v,
        Err(e) => {
            let stable = e.to_string();
            tracing::warn!(
                detail = %e.detail_for_log(),
                kind = %e.data_kind(),
                helper = "balance",
                "ctx.evm.readNative.balance failed"
            );
            return Err(throw_js_error(&ctx, &stable));
        }
    };

    let target = account.to_lowercase();
    let resolved = resolve_block_number(&provider, cfg, block_tag);
    let mut payload = serde_json::json!({
        "helper": "balance",
        "args": [],
        "account": account,
        "block_tag": block_tag_to_json(block_tag),
    });
    if let (Some(n), Some(obj)) = (resolved, payload.as_object_mut()) {
        obj.insert("block_number_resolved".into(), serde_json::Value::from(n));
    }
    evm_reads
        .borrow_mut()
        .push(crate::runtime::EvmReadRecord {
            target,
            payload_json: payload,
        });

    json_to_qjs_value(&ctx, &json).map_err(|e| throw_js_error(&ctx, &e))
}

/// Phase 4 D-07 host binding for `ctx.evm.readNative.blockNumber()`.
/// Returns a JSON Number per D-07. Journals one row with
/// `target="(block_number)"` per D-13.
fn native_block_number_host_binding<'js>(
    ctx: Ctx<'js>,
    provider: Option<&std::sync::Arc<executor_evm::DynProvider>>,
    cfg: &executor_evm::EvmConfig,
    evm_reads: &Rc<RefCell<Vec<crate::runtime::EvmReadRecord>>>,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    let provider = match provider {
        Some(p) => p.clone(),
        None => {
            return Err(throw_js_error(
                &ctx,
                "ctx.evm.readNative.blockNumber not available: no provider configured",
            ));
        }
    };

    let dispatch = executor_evm::native::native_block_number(provider, cfg);
    let result: Result<serde_json::Value, executor_evm::EvmError> =
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => handle.block_on(dispatch),
            Err(_) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| {
                        throw_js_error(
                            &ctx,
                            &format!("evm rpc error: runtime build failed: {e}"),
                        )
                    })?;
                rt.block_on(dispatch)
            }
        };

    let json = match result {
        Ok(v) => v,
        Err(e) => {
            let stable = e.to_string();
            tracing::warn!(
                detail = %e.detail_for_log(),
                kind = %e.data_kind(),
                helper = "blockNumber",
                "ctx.evm.readNative.blockNumber failed"
            );
            return Err(throw_js_error(&ctx, &stable));
        }
    };

    let payload = serde_json::json!({
        "helper": "blockNumber",
        "args": [],
    });
    evm_reads
        .borrow_mut()
        .push(crate::runtime::EvmReadRecord {
            target: "(block_number)".to_string(),
            payload_json: payload,
        });

    json_to_qjs_value(&ctx, &json).map_err(|e| throw_js_error(&ctx, &e))
}

fn throw_js_error(ctx: &Ctx<'_>, msg: &str) -> rquickjs::Error {
    rquickjs::Exception::from_message(ctx.clone(), msg)
        .ok()
        .map(|e| e.throw())
        .unwrap_or(rquickjs::Error::Exception)
}

fn parse_block_tag(v: &rquickjs::Value<'_>) -> Result<executor_evm::read::BlockTag, String> {
    use executor_evm::read::BlockTag;
    if let Some(s) = v.as_string() {
        let s: String = s.to_string().map_err(|e| e.to_string())?;
        match s.as_str() {
            "latest" => Ok(BlockTag::Latest),
            "pending" => Ok(BlockTag::Pending),
            other => Err(format!("blockTag string must be 'latest'|'pending', got {other:?}")),
        }
    } else if let Some(n) = v.as_int() {
        if n < 0 {
            return Err(format!("blockTag number must be non-negative, got {n}"));
        }
        Ok(BlockTag::Number(n as u64))
    } else if let Some(n) = v.as_float() {
        if n < 0.0 || !n.is_finite() {
            return Err(format!("blockTag number must be finite non-negative, got {n}"));
        }
        Ok(BlockTag::Number(n as u64))
    } else {
        Err("blockTag must be 'latest'|'pending'|number".into())
    }
}

fn block_tag_to_json(tag: executor_evm::read::BlockTag) -> serde_json::Value {
    use executor_evm::read::BlockTag;
    match tag {
        BlockTag::Latest => serde_json::Value::String("latest".into()),
        BlockTag::Pending => serde_json::Value::String("pending".into()),
        BlockTag::Number(n) => serde_json::Value::from(n),
    }
}

/// WR-03: resolve `block_number_resolved` for the journal payload (D-13).
///
/// - `BlockTag::Number(n)` → `Some(n)` (verbatim, no extra RPC).
/// - `BlockTag::Latest|Pending` → one extra `eth_blockNumber` round-trip;
///   on failure we log via `tracing::warn!` and return `None` (the field is
///   then omitted from the payload).
fn resolve_block_number(
    provider: &std::sync::Arc<executor_evm::DynProvider>,
    cfg: &executor_evm::EvmConfig,
    tag: executor_evm::read::BlockTag,
) -> Option<u64> {
    use executor_evm::read::BlockTag;
    match tag {
        BlockTag::Number(n) => Some(n),
        BlockTag::Latest | BlockTag::Pending => {
            let dispatch = executor_evm::native::native_block_number(provider.clone(), cfg);
            let result = match tokio::runtime::Handle::try_current() {
                Ok(handle) => handle.block_on(dispatch),
                Err(_) => {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .ok()?;
                    rt.block_on(dispatch)
                }
            };
            match result {
                Ok(serde_json::Value::Number(n)) => n.as_u64(),
                Ok(_) => None,
                Err(e) => {
                    tracing::warn!(
                        detail = %e.detail_for_log(),
                        kind = %e.data_kind(),
                        "block_number_resolved lookup failed; omitting from payload"
                    );
                    None
                }
            }
        }
    }
}

fn json_value_kind(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Inverse of `qjs_value_to_json` for read-contract return values.
fn json_to_qjs_value<'js>(
    ctx: &Ctx<'js>,
    v: &serde_json::Value,
) -> Result<rquickjs::Value<'js>, String> {
    match v {
        serde_json::Value::Null => Ok(rquickjs::Value::new_null(ctx.clone())),
        serde_json::Value::Bool(b) => Ok(rquickjs::Value::new_bool(ctx.clone(), *b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i >= i32::MIN as i64 && i <= i32::MAX as i64 {
                    return Ok(rquickjs::Value::new_int(ctx.clone(), i as i32));
                }
                return Ok(rquickjs::Value::new_float(ctx.clone(), i as f64));
            }
            if let Some(f) = n.as_f64() {
                return Ok(rquickjs::Value::new_float(ctx.clone(), f));
            }
            Err(format!("number not representable: {n}"))
        }
        serde_json::Value::String(s) => Ok(rquickjs::String::from_str(ctx.clone(), s)
            .map_err(|e| format!("string create: {e}"))?
            .into_value()),
        serde_json::Value::Array(arr) => {
            let out = rquickjs::Array::new(ctx.clone()).map_err(|e| e.to_string())?;
            for (i, item) in arr.iter().enumerate() {
                let v = json_to_qjs_value(ctx, item)?;
                out.set(i, v).map_err(|e| e.to_string())?;
            }
            Ok(out.into_value())
        }
        serde_json::Value::Object(obj) => {
            let out = Object::new(ctx.clone()).map_err(|e| e.to_string())?;
            for (k, v) in obj {
                let val = json_to_qjs_value(ctx, v)?;
                out.set(k.as_str(), val).map_err(|e| e.to_string())?;
            }
            Ok(out.into_value())
        }
    }
}

/// Phase 4 D-08 / D-09: action builder kinds.
///
/// Each variant maps 1:1 to one of the Phase-4 `Action` wire variants. Used
/// by [`make_action_builder_closure`] to dispatch to the correct field set
/// + validators (`executor_evm::action::*`).
#[derive(Debug, Clone, Copy)]
enum ActionBuilderKind {
    ContractCall,
    RawCall,
    Erc20Transfer,
    Erc20Approve,
    NativeTransfer,
}

impl ActionBuilderKind {
    fn js_name(self) -> &'static str {
        match self {
            Self::ContractCall => "contractCall",
            Self::RawCall => "rawCall",
            Self::Erc20Transfer => "erc20Transfer",
            Self::Erc20Approve => "erc20Approve",
            Self::NativeTransfer => "nativeTransfer",
        }
    }
}

/// Build a `Fn(Object) -> Value` closure for the given action kind. Each
/// invocation drives one of the [`build_*_action`] free functions which
/// performs D-09 input validation and returns the action JSON object.
fn make_action_builder_closure(
    kind: ActionBuilderKind,
) -> impl for<'js> Fn(rquickjs::Object<'js>) -> rquickjs::Result<rquickjs::Value<'js>> + 'static {
    move |opts: rquickjs::Object<'_>| -> rquickjs::Result<rquickjs::Value<'_>> {
        let ctx = opts.ctx().clone();
        let action_json = match kind {
            ActionBuilderKind::ContractCall => build_contract_call_action(&opts),
            ActionBuilderKind::RawCall => build_raw_call_action(&opts),
            ActionBuilderKind::Erc20Transfer => build_erc20_transfer_action(&opts),
            ActionBuilderKind::Erc20Approve => build_erc20_approve_action(&opts),
            ActionBuilderKind::NativeTransfer => build_native_transfer_action(&opts),
        };
        match action_json {
            Ok(json) => json_to_qjs_value(&ctx, &json).map_err(|e| {
                throw_js_error(&ctx, &format!("ctx.actions.{}: {e}", kind.js_name()))
            }),
            Err(BuilderError::Stable(msg)) => Err(throw_js_error(&ctx, &msg)),
        }
    }
}

/// Wire-safe builder error (HR/MR-01 carry-forward). The string passed to
/// [`throw_js_error`] is what the user-facing JS Error.message exposes; raw
/// `EvmError::detail_for_log` content goes via `tracing::warn!` only.
enum BuilderError {
    Stable(String),
}

fn evm_err_to_builder_error(e: executor_evm::EvmError, helper: &str) -> BuilderError {
    tracing::warn!(
        helper = helper,
        kind = %e.data_kind(),
        detail = %e.detail_for_log(),
        "ctx.actions builder rejected input"
    );
    // Surface the wire-safe Display string. EvmError::Display emits stable
    // taxonomy strings (e.g. "evm encode error: bad_address").
    BuilderError::Stable(format!("ctx.actions.{helper}: {e}"))
}

fn require_string_field<'js>(
    opts: &rquickjs::Object<'js>,
    field: &str,
    helper: &str,
) -> Result<String, BuilderError> {
    // Detect BigInt explicitly so we can emit the stable D-03 rejection
    // message instead of a confused "must be a string" error (Pitfall 2).
    let v: rquickjs::Value<'js> = opts.get(field).map_err(|e| {
        BuilderError::Stable(format!("ctx.actions.{helper}: reading '{field}' failed: {e}"))
    })?;
    if matches!(v.type_of(), rquickjs::Type::BigInt) {
        return Err(BuilderError::Stable(format!(
            "ctx.actions.{helper}: '{field}' must be a decimal string, got BigInt — use ctx.units.parseUnits(...) or pass a literal string"
        )));
    }
    let s = v.as_string().ok_or_else(|| {
        BuilderError::Stable(format!(
            "ctx.actions.{helper}: '{field}' must be a string"
        ))
    })?;
    s.to_string().map_err(|e| {
        BuilderError::Stable(format!("ctx.actions.{helper}: '{field}' utf8 error: {e}"))
    })
}

fn optional_value_field<'js>(
    opts: &rquickjs::Object<'js>,
    field: &str,
    helper: &str,
) -> Result<String, BuilderError> {
    let v: rquickjs::Value<'js> = match opts.get(field) {
        Ok(x) => x,
        Err(_) => return Ok("0".to_string()),
    };
    if v.is_undefined() || v.is_null() {
        return Ok("0".to_string());
    }
    if matches!(v.type_of(), rquickjs::Type::BigInt) {
        return Err(BuilderError::Stable(format!(
            "ctx.actions.{helper}: '{field}' must be a decimal string, got BigInt — use ctx.units.parseUnits(...) or pass a literal string"
        )));
    }
    if let Some(s) = v.as_string() {
        return s.to_string().map_err(|e| {
            BuilderError::Stable(format!("ctx.actions.{helper}: '{field}' utf8 error: {e}"))
        });
    }
    Err(BuilderError::Stable(format!(
        "ctx.actions.{helper}: '{field}' must be a decimal string when present"
    )))
}

/// Extract the `abi` field — accept either a JSON string or a JS array of
/// fragments (mirrors `ctx.evm.readContract` D-05 dual shape).
fn require_abi_field<'js>(
    opts: &rquickjs::Object<'js>,
    helper: &str,
) -> Result<String, BuilderError> {
    let v: rquickjs::Value<'js> = opts.get("abi").map_err(|e| {
        BuilderError::Stable(format!(
            "ctx.actions.{helper}: reading 'abi' failed: {e}"
        ))
    })?;
    if let Some(s) = v.as_string() {
        return s.to_string().map_err(|e| {
            BuilderError::Stable(format!("ctx.actions.{helper}: 'abi' utf8 error: {e}"))
        });
    }
    if v.is_array() || v.is_object() {
        let json = qjs_value_to_json(&v).map_err(|e| {
            BuilderError::Stable(format!("ctx.actions.{helper}: 'abi' walk: {e}"))
        })?;
        return serde_json::to_string(&json).map_err(|e| {
            BuilderError::Stable(format!(
                "ctx.actions.{helper}: 'abi' serialize: {e}"
            ))
        });
    }
    Err(BuilderError::Stable(format!(
        "ctx.actions.{helper}: 'abi' must be a JSON string or an array of fragments"
    )))
}

/// Extract `args` as a JSON array. Missing / undefined / null → empty array.
fn require_args_field<'js>(
    opts: &rquickjs::Object<'js>,
    helper: &str,
) -> Result<Vec<serde_json::Value>, BuilderError> {
    let v: rquickjs::Value<'js> = match opts.get("args") {
        Ok(x) => x,
        Err(_) => return Ok(Vec::new()),
    };
    if v.is_undefined() || v.is_null() {
        return Ok(Vec::new());
    }
    let json = qjs_value_to_json(&v).map_err(|e| {
        BuilderError::Stable(format!("ctx.actions.{helper}: 'args' walk: {e}"))
    })?;
    match json {
        serde_json::Value::Array(a) => Ok(a),
        other => Err(BuilderError::Stable(format!(
            "ctx.actions.{helper}: 'args' must be an array, got {}",
            json_value_kind(&other)
        ))),
    }
}

fn build_contract_call_action(
    opts: &rquickjs::Object<'_>,
) -> Result<serde_json::Value, BuilderError> {
    let helper = "contractCall";
    let address = require_string_field(opts, "address", helper)?;
    let abi = require_abi_field(opts, helper)?;
    let function = require_string_field(opts, "function", helper)?;
    let args = require_args_field(opts, helper)?;
    let value = optional_value_field(opts, "value", helper)?;

    executor_evm::action::validate_address(&address)
        .map_err(|e| evm_err_to_builder_error(e, helper))?;
    executor_evm::action::validate_decimal_amount(&value)
        .map_err(|e| evm_err_to_builder_error(e, helper))?;
    executor_evm::action::dry_run_abi_encode(&abi, &function, &args)
        .map_err(|e| evm_err_to_builder_error(e, helper))?;

    Ok(serde_json::json!({
        "kind": "contract_call",
        "address": address,
        "abi": abi,
        "function": function,
        "args": args,
        "value": value,
    }))
}

fn build_raw_call_action(
    opts: &rquickjs::Object<'_>,
) -> Result<serde_json::Value, BuilderError> {
    let helper = "rawCall";
    let address = require_string_field(opts, "address", helper)?;
    let data = require_string_field(opts, "data", helper)?;
    let value = optional_value_field(opts, "value", helper)?;

    executor_evm::action::validate_address(&address)
        .map_err(|e| evm_err_to_builder_error(e, helper))?;
    executor_evm::action::validate_calldata(&data)
        .map_err(|e| evm_err_to_builder_error(e, helper))?;
    executor_evm::action::validate_decimal_amount(&value)
        .map_err(|e| evm_err_to_builder_error(e, helper))?;

    Ok(serde_json::json!({
        "kind": "raw_call",
        "address": address,
        "data": data,
        "value": value,
    }))
}

fn build_erc20_transfer_action(
    opts: &rquickjs::Object<'_>,
) -> Result<serde_json::Value, BuilderError> {
    let helper = "erc20Transfer";
    let token = require_string_field(opts, "token", helper)?;
    let to = require_string_field(opts, "to", helper)?;
    let amount = require_string_field(opts, "amount", helper)?;

    executor_evm::action::validate_address(&token)
        .map_err(|e| evm_err_to_builder_error(e, helper))?;
    executor_evm::action::validate_address(&to)
        .map_err(|e| evm_err_to_builder_error(e, helper))?;
    executor_evm::action::validate_decimal_amount(&amount)
        .map_err(|e| evm_err_to_builder_error(e, helper))?;

    Ok(serde_json::json!({
        "kind": "erc20_transfer",
        "token": token,
        "to": to,
        "amount": amount,
    }))
}

fn build_erc20_approve_action(
    opts: &rquickjs::Object<'_>,
) -> Result<serde_json::Value, BuilderError> {
    let helper = "erc20Approve";
    let token = require_string_field(opts, "token", helper)?;
    let spender = require_string_field(opts, "spender", helper)?;
    let amount = require_string_field(opts, "amount", helper)?;

    executor_evm::action::validate_address(&token)
        .map_err(|e| evm_err_to_builder_error(e, helper))?;
    executor_evm::action::validate_address(&spender)
        .map_err(|e| evm_err_to_builder_error(e, helper))?;
    executor_evm::action::validate_decimal_amount(&amount)
        .map_err(|e| evm_err_to_builder_error(e, helper))?;

    Ok(serde_json::json!({
        "kind": "erc20_approve",
        "token": token,
        "spender": spender,
        "amount": amount,
    }))
}

fn build_native_transfer_action(
    opts: &rquickjs::Object<'_>,
) -> Result<serde_json::Value, BuilderError> {
    let helper = "nativeTransfer";
    let to = require_string_field(opts, "to", helper)?;
    let value = require_string_field(opts, "value", helper)?;

    executor_evm::action::validate_address(&to)
        .map_err(|e| evm_err_to_builder_error(e, helper))?;
    executor_evm::action::validate_decimal_amount(&value)
        .map_err(|e| evm_err_to_builder_error(e, helper))?;

    Ok(serde_json::json!({
        "kind": "native_transfer",
        "to": to,
        "value": value,
    }))
}

// ─── Phase 4 D-10 / D-11 (Plan 04-04): ctx.units + ctx.address closures ───

/// `ctx.units.parseUnits(amount: string, decimals: number) -> string`.
///
/// BigInt amount input is REJECTED at the JS boundary with a stable D-03
/// message (consistent with the action-builder BigInt rejection). decimals
/// must be a non-negative integer that fits a `u8`; values > 77 are rejected
/// by `executor_evm::units::parse_units`.
fn make_parse_units_closure()
-> impl for<'js> Fn(rquickjs::Value<'js>, rquickjs::Value<'js>) -> rquickjs::Result<rquickjs::Value<'js>>
+ 'static {
    move |amount: rquickjs::Value<'_>, decimals: rquickjs::Value<'_>| {
        let ctx = amount.ctx().clone();
        // BigInt rejected upfront with stable D-03 message (Pitfall 2).
        if matches!(amount.type_of(), rquickjs::Type::BigInt) {
            return Err(throw_js_error(
                &ctx,
                "ctx.units.parseUnits: 'amount' must be a decimal string, got BigInt — pass a literal string",
            ));
        }
        let amount_str = match amount.as_string() {
            Some(s) => s.to_string().map_err(|e| {
                throw_js_error(
                    &ctx,
                    &format!("ctx.units.parseUnits: 'amount' utf8 error: {e}"),
                )
            })?,
            None => {
                return Err(throw_js_error(
                    &ctx,
                    "ctx.units.parseUnits: 'amount' must be a decimal string",
                ));
            }
        };
        let dec_u8 = parse_decimals_arg(&ctx, &decimals, "ctx.units.parseUnits")?;
        match executor_evm::units::parse_units(&amount_str, dec_u8) {
            Ok(u) => {
                let s = u.to_string();
                Ok(rquickjs::String::from_str(ctx.clone(), &s)
                    .map_err(|e| throw_js_error(&ctx, &format!("string alloc: {e}")))?
                    .into_value())
            }
            Err(e) => {
                tracing::warn!(
                    helper = "parseUnits",
                    kind = %e.data_kind(),
                    detail = %e.detail_for_log(),
                    "ctx.units.parseUnits rejected input"
                );
                Err(throw_js_error(
                    &ctx,
                    &format!("ctx.units.parseUnits: {e}"),
                ))
            }
        }
    }
}

/// `ctx.units.formatUnits(value: string, decimals: number) -> string`.
///
/// `value` MUST be a decimal string (BigInt rejected — D-03). `decimals`
/// is `0..=77`; trailing zeros in the fractional part are trimmed.
fn make_format_units_closure()
-> impl for<'js> Fn(rquickjs::Value<'js>, rquickjs::Value<'js>) -> rquickjs::Result<rquickjs::Value<'js>>
+ 'static {
    move |value: rquickjs::Value<'_>, decimals: rquickjs::Value<'_>| {
        let ctx = value.ctx().clone();
        if matches!(value.type_of(), rquickjs::Type::BigInt) {
            return Err(throw_js_error(
                &ctx,
                "ctx.units.formatUnits: 'value' must be a decimal string, got BigInt — pass a literal string",
            ));
        }
        let value_str = match value.as_string() {
            Some(s) => s.to_string().map_err(|e| {
                throw_js_error(
                    &ctx,
                    &format!("ctx.units.formatUnits: 'value' utf8 error: {e}"),
                )
            })?,
            None => {
                return Err(throw_js_error(
                    &ctx,
                    "ctx.units.formatUnits: 'value' must be a decimal string",
                ));
            }
        };
        let dec_u8 = parse_decimals_arg(&ctx, &decimals, "ctx.units.formatUnits")?;
        match executor_evm::units::format_units_from_str(&value_str, dec_u8) {
            Ok(s) => Ok(rquickjs::String::from_str(ctx.clone(), &s)
                .map_err(|e| throw_js_error(&ctx, &format!("string alloc: {e}")))?
                .into_value()),
            Err(e) => {
                tracing::warn!(
                    helper = "formatUnits",
                    kind = %e.data_kind(),
                    detail = %e.detail_for_log(),
                    "ctx.units.formatUnits rejected input"
                );
                Err(throw_js_error(
                    &ctx,
                    &format!("ctx.units.formatUnits: {e}"),
                ))
            }
        }
    }
}

/// Helper: parse a JS decimals arg as `u8`. Rejects negatives, non-finite,
/// non-integer, and values > 255 (the inner module further caps at 77).
fn parse_decimals_arg(
    ctx: &rquickjs::Ctx<'_>,
    v: &rquickjs::Value<'_>,
    helper: &str,
) -> rquickjs::Result<u8> {
    let n: f64 = if let Some(i) = v.as_int() {
        i as f64
    } else if let Some(f) = v.as_float() {
        f
    } else {
        return Err(throw_js_error(
            ctx,
            &format!("{helper}: 'decimals' must be a number"),
        ));
    };
    if !n.is_finite() || n < 0.0 || n.fract() != 0.0 || n > 255.0 {
        return Err(throw_js_error(
            ctx,
            &format!("{helper}: 'decimals' must be a non-negative integer ≤ 255"),
        ));
    }
    Ok(n as u8)
}

/// `ctx.address.isAddress(s: any) -> boolean`. Total — never throws.
/// Non-strings return `false`.
fn make_is_address_closure()
-> impl for<'js> Fn(rquickjs::Ctx<'js>, rquickjs::Value<'js>) -> rquickjs::Result<rquickjs::Value<'js>>
+ 'static {
    move |ctx: rquickjs::Ctx<'_>, v: rquickjs::Value<'_>| {
        let result = match v.as_string() {
            Some(js) => match js.to_string() {
                Ok(s) => executor_evm::address::is_address(&s),
                Err(_) => false,
            },
            None => false,
        };
        Ok(rquickjs::Value::new_bool(ctx, result))
    }
}

/// `ctx.address.checksum(s: string) -> string`. Throws on invalid input.
fn make_address_checksum_closure()
-> impl for<'js> Fn(rquickjs::Value<'js>) -> rquickjs::Result<rquickjs::Value<'js>>
+ 'static {
    move |v: rquickjs::Value<'_>| {
        let ctx = v.ctx().clone();
        let s = match v.as_string() {
            Some(js) => js.to_string().map_err(|e| {
                throw_js_error(
                    &ctx,
                    &format!("ctx.address.checksum: utf8 error: {e}"),
                )
            })?,
            None => {
                return Err(throw_js_error(
                    &ctx,
                    "ctx.address.checksum: argument must be a string",
                ));
            }
        };
        match executor_evm::address::checksum(&s) {
            Ok(out) => Ok(rquickjs::String::from_str(ctx.clone(), &out)
                .map_err(|e| throw_js_error(&ctx, &format!("string alloc: {e}")))?
                .into_value()),
            Err(e) => {
                tracing::warn!(
                    helper = "checksum",
                    kind = %e.data_kind(),
                    detail = %e.detail_for_log(),
                    "ctx.address.checksum rejected input"
                );
                Err(throw_js_error(
                    &ctx,
                    &format!("ctx.address.checksum: {e}"),
                ))
            }
        }
    }
}

/// Walk a `rquickjs::Value` and produce a `serde_json::Value`. Returns
/// `Err(detail)` for shapes we cannot represent (functions, BigInts,
/// Symbols, Promises). Plan 03-03 layers semantic `Action[]`/`"noop"`
/// validation on top of the JSON.
fn qjs_value_to_json(value: &Value<'_>) -> Result<serde_json::Value, String> {
    use rquickjs::Type;
    match value.type_of() {
        Type::Uninitialized | Type::Undefined | Type::Null => Ok(serde_json::Value::Null),
        Type::Bool => Ok(serde_json::Value::Bool(
            value.as_bool().ok_or_else(|| "bool: type mismatch".to_string())?,
        )),
        Type::Int => Ok(serde_json::Value::from(
            value
                .as_int()
                .ok_or_else(|| "int: type mismatch".to_string())?,
        )),
        Type::Float => {
            let n = value
                .as_float()
                .ok_or_else(|| "float: type mismatch".to_string())?;
            serde_json::Number::from_f64(n)
                .map(serde_json::Value::Number)
                .ok_or_else(|| format!("non-finite float not representable in JSON: {n}"))
        }
        Type::String => {
            let s = value
                .as_string()
                .ok_or_else(|| "string: type mismatch".to_string())?
                .to_string()
                .map_err(|e| format!("string utf8: {e}"))?;
            Ok(serde_json::Value::String(s))
        }
        Type::Array => {
            let arr = value
                .as_array()
                .ok_or_else(|| "array: type mismatch".to_string())?;
            let mut out = Vec::with_capacity(arr.len());
            for i in 0..arr.len() {
                let item: Value = arr
                    .get::<Value>(i)
                    .map_err(|e| format!("array[{i}]: {e}"))?;
                out.push(qjs_value_to_json(&item)?);
            }
            Ok(serde_json::Value::Array(out))
        }
        Type::Object => {
            let obj = value
                .as_object()
                .ok_or_else(|| "object: type mismatch".to_string())?;
            let mut map = serde_json::Map::new();
            for prop in obj.props::<String, Value>() {
                let (k, v) = prop.map_err(|e| format!("object iter: {e}"))?;
                map.insert(k, qjs_value_to_json(&v)?);
            }
            Ok(serde_json::Value::Object(map))
        }
        Type::Function | Type::Constructor => {
            Err("functions are not serializable in strategy returns".into())
        }
        Type::Symbol => Err("symbols are not serializable in strategy returns".into()),
        Type::BigInt => {
            Err("BigInt is not supported in strategy returns (Pitfall 8)".into())
        }
        Type::Promise => Err(
            "promise return values are not supported in v1; strategies must be synchronous"
                .into(),
        ),
        Type::Exception => Err("uncaught exception value cannot be returned".into()),
        Type::Module => Err("module values cannot be returned".into()),
        Type::Proxy => Err("proxy values cannot be serialized in strategy returns".into()),
        Type::Unknown => Err("unknown JS value type cannot be returned".into()),
    }
}
