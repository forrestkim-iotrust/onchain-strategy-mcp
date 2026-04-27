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
}

/// In-memory `CtxHost` implementation used by Phase-3 unit tests and as the
/// type Plan 03-02 will replace at the MCP boundary.
#[derive(Debug, Default)]
pub struct CtxStub {
    pub strategy_id: String,
    pub strategy_name: String,
    pub run_id: String,
    pub logs: Vec<String>,
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
            ctx_obj
                .set("evm", evm_obj)
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
            Ok(handle) => tokio::task::block_in_place(|| {
                handle.block_on(executor_evm::read_contract(provider, cfg, input))
            }),
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
                rt.block_on(executor_evm::read_contract(provider, cfg, input))
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
    let payload = serde_json::json!({
        "helper": "readContract",
        "args": args_arr,
        "function": function_name,
        "address": address,
        "block_tag": block_tag_to_json(block_tag),
    });
    evm_reads
        .borrow_mut()
        .push(crate::runtime::EvmReadRecord {
            target,
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
