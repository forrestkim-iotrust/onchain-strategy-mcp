//! Alloy provider construction (Phase 4 D-04).
//!
//! Single shared `Arc<DynProvider>` per `ExecutorServer`, lazy-init on first
//! `ctx.evm.*` call. The Provider is `Send + Sync + Clone` and may be moved
//! into `spawn_blocking` closures. **The Provider is NEVER exposed to JS as
//! a value** — strategy-js host bindings clone the Arc into closures only.

use std::sync::Arc;

use alloy::providers::{DynProvider, Provider, ProviderBuilder};

use crate::{EvmConfig, EvmError};

/// Build a shared HTTP provider. The returned `Arc<DynProvider>` is
/// `Send + Sync + Clone`.
pub fn build_provider(cfg: &EvmConfig) -> Result<Arc<DynProvider>, EvmError> {
    let provider = ProviderBuilder::new()
        .connect_http(cfg.rpc_url.clone())
        .erased();
    Ok(Arc::new(provider))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_provider_succeeds_with_default_config() {
        let cfg = EvmConfig::default();
        let provider = build_provider(&cfg).expect("build_provider");
        // Send + Sync + Clone witnesses (compile-time):
        fn assert_send_sync_clone<T: Send + Sync + Clone>(_: &T) {}
        assert_send_sync_clone(&provider);
        // Provider is built but no network call has happened yet — server
        // boot independent of devnet liveness (D-04).
        let _ = provider;
    }
}
