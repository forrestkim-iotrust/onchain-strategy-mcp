//! Non-secret local signer configuration.

use crate::SignerError;

/// v1.3: signer backend selector. `Env` preserves Phase 6 behaviour
/// (private key read from an environment variable). `Keychain` reads
/// the key from the OS keychain (service="onchain-strategy-mcp",
/// account=`key_id`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignerBackend {
    Env,
    Keychain,
}

impl SignerBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::Keychain => "keychain",
        }
    }
}

/// Configuration for resolving a local signer.
///
/// Backwards compat: when `backend = Env` the existing
/// `private_key_env` path is used. When `backend = Keychain` the
/// `key_id` identifies the OS keychain account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSignerConfig {
    /// Name of the environment variable containing the hex EVM private key.
    /// Used when `backend = Env`. Kept for backwards compat even when
    /// `backend = Keychain` (callers may leave it unset / blank).
    pub private_key_env: String,
    /// Receipt wait timeout used by the managed execution boundary.
    pub receipt_timeout_ms: u64,
    /// v1.3 backend selector.
    pub backend: SignerBackend,
    /// v1.3 keychain account / key id. Defaults to "default".
    pub key_id: String,
}

impl LocalSignerConfig {
    /// Create signer config from an environment-variable name.
    ///
    /// The raw private-key value is intentionally not accepted here; it is
    /// resolved only by [`crate::LocalSignerHandle::from_env`].
    pub fn new(
        private_key_env: impl Into<String>,
        receipt_timeout_ms: u64,
    ) -> Result<Self, SignerError> {
        let private_key_env = private_key_env.into();
        if private_key_env.trim().is_empty() || receipt_timeout_ms == 0 {
            return Err(SignerError::NotConfigured);
        }
        Ok(Self {
            private_key_env,
            receipt_timeout_ms,
            backend: SignerBackend::Env,
            key_id: "default".to_string(),
        })
    }

    /// v1.3: build a keychain-backed signer config.
    pub fn new_keychain(
        key_id: impl Into<String>,
        receipt_timeout_ms: u64,
    ) -> Result<Self, SignerError> {
        let key_id = key_id.into();
        if key_id.trim().is_empty() || receipt_timeout_ms == 0 {
            return Err(SignerError::NotConfigured);
        }
        Ok(Self {
            // Kept non-empty for backwards-compat invariants; never read
            // when backend = Keychain.
            private_key_env: String::new(),
            receipt_timeout_ms,
            backend: SignerBackend::Keychain,
            key_id,
        })
    }
}
