#![allow(dead_code, unreachable_pub)]
//! Shared test helpers for `executor-evm` integration tests.
//!
//! `anvil_fixture` is gated on `anvil-tests` (the cargo feature that opts
//! tests in to spawning anvil) OR on `test-fixtures` (Phase 5/6 may consume
//! the fixture as a dev-dep — D-14).

#[cfg(any(feature = "anvil-tests", feature = "test-fixtures"))]
pub mod anvil_fixture;
