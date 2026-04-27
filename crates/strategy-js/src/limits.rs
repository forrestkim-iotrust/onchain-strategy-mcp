//! D-03 resource budgets. **Constants only** (no runtime config in v1) so
//! the agent-facing contract is mechanical: every strategy gets the same
//! 2-second / 64-MiB / 1-MiB envelope.
//!
//! ## Pitfall 3 — `set_memory_limit(0)` means UNLIMITED, not zero.
//! [VERIFIED docs.rs/rquickjs/latest/rquickjs/struct.Runtime.html#method.set_memory_limit]
//! These constants MUST be non-zero. Do NOT introduce an `Option<usize>`
//! "use default" pattern that could unwrap to 0.

/// Strategy wall-clock budget in milliseconds (D-03 — 2 seconds).
/// The interrupt handler is polled between bytecode instructions; once
/// `Instant::now() >= start + WALL_CLOCK_MS`, the interpreter raises an
/// uncatchable exception.
pub const WALL_CLOCK_MS: u64 = 2_000;

/// Heap budget in bytes (D-03 — 64 MiB). Fed directly to
/// `rquickjs::Runtime::set_memory_limit`. Pitfall 3: must NOT be zero.
pub const MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;

/// GC trigger threshold in bytes (D-03 — 8 MiB, 1/8 of heap). Fed to
/// `Runtime::set_gc_threshold`.
pub const GC_THRESHOLD_BYTES: usize = 8 * 1024 * 1024;

/// Max C-stack size for QuickJS in bytes (D-03 — 1 MiB; default rquickjs
/// is 256 KiB which is too tight for legitimate recursion). Fed to
/// `Runtime::set_max_stack_size`.
pub const MAX_STACK_BYTES: usize = 1024 * 1024;

// Compile-time guards for D-03 invariants. Using `const { assert!(..) }`
// keeps clippy::assertions_on_constants happy and shifts the check to
// compile time (Pitfall 3 — `set_memory_limit(0)` means UNLIMITED, not zero).
const _: () = {
    assert!(MEMORY_LIMIT_BYTES > 0, "MEMORY_LIMIT_BYTES must be non-zero (Pitfall 3)");
    assert!(GC_THRESHOLD_BYTES > 0, "GC_THRESHOLD_BYTES must be non-zero");
    assert!(MAX_STACK_BYTES > 0, "MAX_STACK_BYTES must be non-zero");
    assert!(WALL_CLOCK_MS > 0, "WALL_CLOCK_MS must be non-zero");
    assert!(
        GC_THRESHOLD_BYTES < MEMORY_LIMIT_BYTES,
        "GC threshold must be smaller than the heap cap"
    );
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_match_d03_constants() {
        // Pin exact D-03 values so a future careless edit fails loudly.
        assert_eq!(WALL_CLOCK_MS, 2_000);
        assert_eq!(MEMORY_LIMIT_BYTES, 64 * 1024 * 1024);
        assert_eq!(GC_THRESHOLD_BYTES, 8 * 1024 * 1024);
        assert_eq!(MAX_STACK_BYTES, 1024 * 1024);
    }
}
