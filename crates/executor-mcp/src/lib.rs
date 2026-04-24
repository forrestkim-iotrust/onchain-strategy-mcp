#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
//! `executor-mcp` — stdio MCP server.
//!
//! - `config` loads `config.toml` (Phase 1: `logging.level` only).
//! - `logging` sets up a stderr-only tracing subscriber so stdout stays pure
//!   JSON-RPC (D-05).
//! - `errors` exposes `unimplemented_err` for the 4 write-capable Phase 1 tools.
//! - `server` defines `ExecutorServer` + its `ServerHandler` impl.
//! - `tools` hosts the `#[tool_router]` impl block with the 8 Phase 1 tools.
//!
//! Plan 03 will add a `prompts` module and extend `server` with
//! `#[prompt_handler]` alongside `#[tool_handler]`.

pub mod config;
pub mod errors;
pub mod logging;
pub mod server;
pub mod tools;

pub use server::ExecutorServer;
