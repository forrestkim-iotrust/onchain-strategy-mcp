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

pub mod action;
pub mod address;
pub mod config;
pub mod dyn_abi;
pub mod erc20;
pub mod error;
pub mod native;
pub mod provider;
pub mod read;
pub mod units;

pub use action::{
    dry_run_abi_encode, validate_abi_size, validate_address, validate_calldata,
    validate_decimal_amount,
};
pub use address::{ZERO_ADDRESS, checksum as address_checksum, is_address};
pub use config::EvmConfig;
pub use erc20::{
    ERC20_ABI, erc20_allowance, erc20_balance_of, erc20_decimals, erc20_name, erc20_symbol,
    erc20_total_supply,
};
pub use error::EvmError;
pub use native::{native_balance, native_block_number};
pub use provider::build_provider;
pub use read::{BlockTag, ReadContractInput, read_contract};
pub use units::{format_units, format_units_from_str, parse_units};

// Re-export the alloy `DynProvider` alias so downstream crates
// (executor-mcp, strategy-js) can name `Arc<DynProvider>` without
// taking a direct alloy dependency (Phase 4 D-02 isolation).
pub use alloy::providers::DynProvider;
