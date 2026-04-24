#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
//! Domain types and trait boundaries for the onchain-strategy-mcp runtime.
//!
//! Phase 1 exposes only the schema structs and the error enum. Subsequent
//! phases layer on persistence (`executor-state`), signing (`executor-signer`),
//! and EVM/JS runtime crates that depend on these types.

pub mod error;
pub mod schema;
