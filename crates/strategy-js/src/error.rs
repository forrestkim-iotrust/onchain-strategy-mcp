//! Typed runtime errors. MCP error-code mapping (D-07) lives in
//! `executor-mcp::errors::map_runtime_error` (Plan 03-03), which converts
//! these into `-32011` / `-32017` / `-32018` MCP error responses.

/// Runtime/sandbox failure mode. The `Exception` variant carries the JS-level
/// message; OOM / Timeout / StackOverflow / InvalidOutput are pre-classified
/// so `executor-mcp` can surface a typed `data.kind` field to agents.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// Wall-clock interrupt fired (D-03 — `WALL_CLOCK_MS`).
    #[error("strategy timeout: wall-clock budget exceeded")]
    Timeout,

    /// Heap allocation past `MEMORY_LIMIT_BYTES` (D-03).
    #[error("strategy out-of-memory: heap budget exceeded")]
    Oom,

    /// C-stack overflow past `MAX_STACK_BYTES` (D-03).
    #[error("strategy stack overflow: max stack size exceeded")]
    StackOverflow,

    /// JS-level uncaught exception thrown by the strategy code.
    #[error("strategy exception: {0}")]
    Exception(String),

    /// Strategy violates D-05 entry-point shape, D-10 promise rejection,
    /// or returns a value that does not validate as `"noop"` / `Action[]`.
    /// The `detail` field is agent-readable.
    #[error("strategy invalid output: {detail}")]
    InvalidOutput { detail: String },

    /// rquickjs Runtime / Context construction failed (very rare — typically
    /// out-of-memory at host level, distinct from sandbox OOM).
    #[error("strategy engine init failed: {0}")]
    EngineInit(String),
}

impl From<rquickjs::Error> for RuntimeError {
    fn from(e: rquickjs::Error) -> Self {
        // Default conversion — `Sandbox::execute` calls `classify_qjs_error`
        // first to apply deadline-hit + memory-limit heuristics before
        // falling back to this generic mapping.
        RuntimeError::Exception(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_messages_are_human_readable() {
        assert_eq!(
            RuntimeError::Timeout.to_string(),
            "strategy timeout: wall-clock budget exceeded"
        );
        assert_eq!(
            RuntimeError::Oom.to_string(),
            "strategy out-of-memory: heap budget exceeded"
        );
        assert_eq!(
            RuntimeError::StackOverflow.to_string(),
            "strategy stack overflow: max stack size exceeded"
        );
        let inv = RuntimeError::InvalidOutput {
            detail: "promise return".into(),
        };
        assert!(inv.to_string().contains("promise return"));
    }

    #[test]
    fn from_rquickjs_error_produces_exception_variant() {
        // Trigger a real rquickjs::Error via a syntax-error eval, which is the
        // most reliable cross-version way to obtain an `Error` value (avoiding
        // dependence on the public-ness of specific Error variants in 0.11).
        let rt = rquickjs::Runtime::new().expect("runtime");
        let ctx = rquickjs::Context::base(&rt).expect("ctx");
        let qjs_err: Option<rquickjs::Error> = ctx.with(|c| {
            // Deliberately invalid syntax:
            let r: rquickjs::Result<rquickjs::Value> = c.eval(b"@@@".as_ref());
            r.err()
        });
        let qjs_err = qjs_err.expect("expected a syntax error from rquickjs");
        let rt_err: RuntimeError = qjs_err.into();
        assert!(
            matches!(rt_err, RuntimeError::Exception(_)),
            "expected Exception, got: {rt_err:?}"
        );
    }
}
