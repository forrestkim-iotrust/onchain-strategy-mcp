//! Core error type — Phase 2+ adds variants as domain operations come online.

use thiserror::Error;

/// Domain errors surfaced by `executor-core` and adjacent crates.
///
/// Phase 1 only needs a single variant so the type name / import path is
/// stable. Later phases add persistence, simulation, and signing variants
/// without needing to rename the enum.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
}
