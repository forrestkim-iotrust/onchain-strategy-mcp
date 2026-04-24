//! Tool / prompt input schemas shared across the runtime.
//!
//! Task 1 wires up only the `execution` submodule so `executor-signer` can
//! reference `SignedTransaction`. Task 2 adds `strategy`, `action`, `policy`,
//! and `prompt_args` with the real `JsonSchema`-derived structs.

pub mod execution;
