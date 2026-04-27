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
pub trait CtxHost {
    fn strategy_id(&self) -> &str;
    fn strategy_name(&self) -> &str;
    fn run_id(&self) -> &str;
    fn now_millis(&self) -> i64;
    fn append_log(&mut self, message: String);
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

        // Snapshot host fields BEFORE `ctx.with` — closures must own their
        // captures (rquickjs `Function::new` requires `'js`, not `&'js mut H`).
        let strategy_id_owned = host.strategy_id().to_string();
        let strategy_name_owned = host.strategy_name().to_string();
        let run_id_owned = host.run_id().to_string();
        let now_value = host.now_millis() as f64;

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
