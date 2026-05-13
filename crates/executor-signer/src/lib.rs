#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
//! Signer boundary for Phase 6 local managed execution.

pub mod config;
pub mod error;
pub mod keychain;
pub mod local;

pub use config::{LocalSignerConfig, SignerBackend};
pub use error::SignerError;
pub use keychain::{KEYCHAIN_SERVICE, generate_burner, load_from_keychain, store_in_keychain};
pub use local::{LocalExecutionReceipt, LocalPendingExecution, LocalReceiptStatus, LocalSignerHandle};

use executor_core::schema::execution::SignedTransaction;

/// Marker trait for signer implementations behind the signer crate boundary.
pub trait Signer: Send + Sync {}

// Keep the Phase-1 execution schema link available for downstream compatibility.
#[doc(hidden)]
pub type _SignedTransactionAlias = SignedTransaction;
