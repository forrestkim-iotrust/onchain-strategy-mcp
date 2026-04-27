#![allow(dead_code, unreachable_pub)]
//! Shared test helpers for `executor-state` integration tests (D-08b).

use executor_state::StateStore;
use std::path::Path;

pub fn fresh_memory_store() -> StateStore {
    StateStore::open(Path::new(":memory:")).expect("open :memory: store")
}

// `seed_strategies()` is added in Task 2 once `register_strategy()` exists.
