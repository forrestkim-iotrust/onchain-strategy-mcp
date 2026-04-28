//! Stable non-secret signer errors.

/// Signer boundary errors that never include private-key material.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum SignerError {
    /// No signer env-var name was configured.
    #[error("local signer is not configured; set [signer].private_key_env")]
    NotConfigured,
    /// Config named an env var, but that var is absent at runtime.
    #[error("missing local signer private-key environment variable {env}")]
    MissingPrivateKeyEnv { env: String },
    /// Config named an env var, but its value could not be parsed as a key.
    #[error("invalid local signer private key in environment variable {env}")]
    InvalidPrivateKey { env: String },
    /// Other non-secret signer configuration issue.
    #[error("local signer config error: {detail}")]
    Config { detail: String },
}
