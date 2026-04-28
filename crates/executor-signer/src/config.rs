//! Non-secret local signer configuration.

use crate::SignerError;

/// Configuration for resolving a local signer from an environment variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSignerConfig {
    /// Name of the environment variable containing the hex EVM private key.
    pub private_key_env: String,
    /// Receipt wait timeout used by the managed execution boundary.
    pub receipt_timeout_ms: u64,
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
        if private_key_env.trim().is_empty() {
            return Err(SignerError::NotConfigured);
        }
        Ok(Self {
            private_key_env,
            receipt_timeout_ms,
        })
    }
}
