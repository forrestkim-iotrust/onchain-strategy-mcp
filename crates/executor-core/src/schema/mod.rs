//! Tool / prompt input schemas shared across the runtime.
//!
//! Each submodule owns the `schemars::JsonSchema`-derived structs that the
//! MCP server binds to tool names in Plan 02.

pub mod action;
pub mod execution;
pub mod policy;
pub mod prompt_args;
pub mod strategy;
pub mod trigger;
