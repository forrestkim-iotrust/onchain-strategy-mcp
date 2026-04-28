#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
//! Signer boundary for Phase 6 local managed execution.

pub mod config;
pub mod error;
pub mod local;

pub use config::LocalSignerConfig;
pub use error::SignerError;
pub use local::LocalSignerHandle;

use executor_core::schema::execution::SignedTransaction;

/// Marker trait for signer implementations behind the signer crate boundary.
pub trait Signer: Send + Sync {}

// Keep the Phase-1 execution schema link available for downstream compatibility.
#[doc(hidden)]
pub type _SignedTransactionAlias = SignedTransaction;
