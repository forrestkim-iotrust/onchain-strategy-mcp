#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
//! `executor-evm` — alloy-backed EVM read/action infrastructure for Phase 4.
//!
//! - `provider`: Arc<DynProvider> construction; lazy-init owned by
//!   `ExecutorServer`.
//! - `dyn_abi`: BigInt-bridged JS-arg ↔ DynSolValue conversion (Phase 4 D-03).
//! - `read`: `read_contract` entry point used by ctx.evm.readContract.
//! - `error`: typed `EvmError` (mapped to -32017 with extended `data.kind`
//!   taxonomy at the MCP boundary — Phase 4 D-12).
//!
//! ## Sandbox boundary
//! strategy-js stays alloy-free (Phase 4 D-02 isolation). The host bindings
//! for `ctx.evm.*` live in `strategy-js::sandbox`; their bodies delegate to
//! functions in this crate. The Provider is NEVER exposed to JS as a value.

pub mod config;
pub mod dyn_abi;
pub mod error;
pub mod provider;
pub mod read;

pub use config::EvmConfig;
pub use error::EvmError;
pub use provider::build_provider;
pub use read::{BlockTag, ReadContractInput, read_contract};

// Re-export the alloy `DynProvider` alias so downstream crates
// (executor-mcp, strategy-js) can name `Arc<DynProvider>` without
// taking a direct alloy dependency (Phase 4 D-02 isolation).
pub use alloy::providers::DynProvider;
