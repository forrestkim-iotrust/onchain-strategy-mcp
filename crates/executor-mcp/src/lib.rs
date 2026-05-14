#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
//! `executor-mcp` — stdio MCP server.
//!
//! - `config` loads `config.toml` (Phase 1: `logging.level` only).
//! - `logging` sets up a stderr-only tracing subscriber so stdout stays pure
//!   JSON-RPC (D-05).
//! - `errors` exposes `unimplemented_err` for the 4 write-capable Phase 1 tools.
//! - `server` defines `ExecutorServer` + its `ServerHandler` impl with
//!   `#[tool_handler]` + `#[prompt_handler]` on one block (Pitfall 6).
//! - `tools` hosts the `#[tool_router]` impl block with the 8 Phase 1 tools.
//! - `prompts` hosts the `#[prompt_router]` impl block with the 2 placeholder
//!   authoring/review prompts.
//! - `resources` hosts the resource template + read_resource helpers invoked by
//!   the `ServerHandler` block in `server`.

pub mod alignment;
pub mod config;
pub mod contracts_touched;
pub mod deploy_delegate;
pub mod errors;
pub mod init;
pub mod logging;
pub mod policy_boot;
pub mod policy_diff;
pub mod prompts;
pub mod records;
pub mod resources;
pub mod server;
pub mod tools;
pub mod triggers;
pub mod validation;
// v1.6 Track 6A: local web UI HTTP server (loopback-only, observation-only).
pub mod web;

pub use server::ExecutorServer;
