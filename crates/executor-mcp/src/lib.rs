#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
//! `executor-mcp` — stdio MCP server entry.
//!
//! Plan 01-02 Task 1 adds `config` + `logging`. Task 2 will add `errors`,
//! `server`, and `tools` modules and re-export `ExecutorServer`.

pub mod config;
pub mod logging;
