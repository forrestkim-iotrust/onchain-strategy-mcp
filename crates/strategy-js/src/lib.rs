#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
//! `strategy-js` — sandboxed JavaScript runtime for Phase-3 strategy execution.
//!
//! Wraps rquickjs 0.11 (QuickJS-NG) under three hard resource budgets
//! (D-03: 2-second wall-clock, 64 MiB heap, 1 MiB stack) and a deny-by-default
//! intrinsic surface (D-11: no console / fetch / setTimeout / require /
//! process / fs / network).
//!
//! Phase-3 scope:
//! - `error`: typed [`RuntimeError`] (D-07 maps these to MCP error codes
//!   in `executor-mcp::errors`, Plan 03-03).
//! - `limits`: D-03 constants, plus the Pitfall 3 sentinel rule
//!   (`set_memory_limit(0)` means UNLIMITED in rquickjs — never use 0).
//! - `sandbox`: synchronous [`Sandbox::execute`] entry point. Caller wraps
//!   the call in `tokio::task::spawn_blocking`; rquickjs `Runtime` is
//!   `!Sync` without the `parallel` feature, so we construct a fresh
//!   Runtime + Context per invocation (RESEARCH Concurrency Plan).
//!
//! Phase-4 will extend `sandbox` with `ctx.evm.*` injection without
//! breaking the Phase-3 [`CtxHost`] trait.

pub mod error;
pub mod limits;
pub mod sandbox;

pub use error::RuntimeError;
pub use sandbox::{CtxHost, CtxStub, Sandbox};
