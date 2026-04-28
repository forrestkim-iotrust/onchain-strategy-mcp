#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
//! `executor-policy` — alloy-free TOML policy DSL parser and deny-by-default
//! evaluator scaffold (Phase 5 D-01 / D-20).
//!
//! - `error`:    typed [`PolicyError`]; wire-safe `Display` (Phase 4 MR-01 / BR-01 carry-forward).
//! - `model`:    [`PolicyConfig`] schema (chains/contracts/selectors/native_value/erc20_spend/raw_call).
//! - `decision`: [`Decision`] input shape + [`DecisionVerdict`] result.
//! - `selector`: 4-byte selector extraction (POL-03 helper).
//!
//! ## Boundary
//!
//! This crate NEVER imports `alloy` (the umbrella crate). Only
//! `alloy-primitives` for `Address` / `U256` decimal-string parsing. Plan 05-03
//! lands the `eval` and `load` module bodies; this Plan (05-01) lands the
//! scaffolding only.

pub mod decision;
pub mod error;
pub mod model;
pub mod selector;

pub use decision::{Decision, DecisionVerdict, NormalizedActionKindCopy};
pub use error::PolicyError;
pub use model::PolicyConfig;
pub use selector::{extract_selector, selector_to_hex};
