//! Manual trigger — placeholder.
//!
//! Manual triggers have **no background loop**. They are driven by the MCP
//! `strategy_run` tool synthesising a `TriggerEvent` and pushing it into the
//! shared dispatcher channel. Stream C owns that wiring; this file exists so
//! the `triggers/workers/` module tree mirrors the design doc and so future
//! kinds slot in next to it without churn.
