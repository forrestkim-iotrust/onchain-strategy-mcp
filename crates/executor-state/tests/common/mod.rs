#![allow(dead_code, unreachable_pub)]
//! Shared test helpers for `executor-state` integration tests (D-08b).

use executor_state::{RegisterOutcome, StateStore};
use std::path::Path;

pub fn fresh_memory_store() -> StateStore {
    StateStore::open(Path::new(":memory:")).expect("open :memory: store")
}

pub fn seed_strategies(store: &mut StateStore, n: usize) -> Vec<String> {
    (0..n)
        .map(|i| {
            let source = format!("// strategy {i}\n");
            let name = format!("s{i}");
            match store
                .register_strategy(&name, &source, None, None)
                .expect("register seed")
            {
                RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
            }
        })
        .collect()
}
