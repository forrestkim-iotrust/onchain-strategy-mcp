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
    /// Broadcast failed before a transaction hash was returned.
    #[error("local signer transaction broadcast failed")]
    BroadcastFailed,
    /// Receipt wait timed out after a transaction hash was returned.
    #[error("local signer receipt timeout for transaction {tx_hash}")]
    ReceiptTimeout { tx_hash: String },
    /// Receipt could not be found after pending transaction confirmation.
    #[error("local signer receipt missing")]
    ReceiptMissing,
    /// Receipt wait failed for a non-timeout reason.
    #[error("local signer receipt wait failed")]
    ReceiptFailed,
    /// Other non-secret signer configuration issue.
    #[error("local signer config error: {detail}")]
    Config { detail: String },
}

impl SignerError {
    /// Stable execution error kind for persistence and MCP-facing fields.
    pub fn execution_error_kind(&self) -> &'static str {
        match self {
            Self::NotConfigured => "signer_not_configured",
            Self::MissingPrivateKeyEnv { .. } => "signer_not_configured",
            Self::InvalidPrivateKey { .. } => "invalid_private_key",
            Self::BroadcastFailed => "broadcast_failed",
            Self::ReceiptTimeout { .. } => "receipt_timeout",
            Self::ReceiptMissing => "receipt_missing",
            Self::ReceiptFailed => "receipt_failed",
            Self::Config { .. } => "signer_not_configured",
        }
    }
}
