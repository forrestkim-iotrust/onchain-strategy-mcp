//! Manual trigger — no background loop.
//!
//! Manual triggers fire only when the MCP `strategy_run` tool (or a future
//! `trigger_fire`-style tool) synthesizes a `TriggerEvent` directly onto the
//! dispatcher channel. There is no worker to spawn; `WorkerPool::spawn`
//! treats `TriggerKind::Manual` as a no-op.
