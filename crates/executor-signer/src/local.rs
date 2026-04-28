//! Local Alloy private-key signer boundary.

use std::{str::FromStr, time::Duration};

use alloy::{
    network::{Ethereum, ReceiptResponse},
    providers::{PendingTransactionBuilder, Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
    signers::{Signer as AlloySigner, local::PrivateKeySigner},
};
use reqwest::Url;
use alloy_primitives::{Address, B256};

use crate::{LocalSignerConfig, SignerError};

/// In-memory local signer handle.
///
/// The handle intentionally exposes only signer address accessors; private-key
/// material remains inside Alloy's `PrivateKeySigner`.
#[derive(Clone)]
pub struct LocalSignerHandle {
    signer: PrivateKeySigner,
    signer_address: Address,
}

#[derive(Debug)]
pub struct LocalPendingExecution {
    pub tx_hash: B256,
    pending: PendingTransactionBuilder<Ethereum>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalExecutionReceipt {
    pub tx_hash: B256,
    pub receipt_status: LocalReceiptStatus,
    pub gas_used: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalReceiptStatus {
    Success,
    Reverted,
}

impl LocalReceiptStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Reverted => "reverted",
        }
    }
}

impl LocalSignerHandle {
    /// Resolve and parse the configured private key from the process environment.
    pub fn from_env(config: &LocalSignerConfig, chain_id: u64) -> Result<Self, SignerError> {
        let raw = std::env::var(&config.private_key_env).map_err(|_| {
            SignerError::MissingPrivateKeyEnv {
                env: config.private_key_env.clone(),
            }
        })?;
        let signer = PrivateKeySigner::from_str(&raw)
            .map_err(|_| SignerError::InvalidPrivateKey {
                env: config.private_key_env.clone(),
            })?
            .with_chain_id(Some(chain_id));
        let signer_address = signer.address();
        Ok(Self {
            signer,
            signer_address,
        })
    }

    #[doc(hidden)]
    pub fn __test_from_private_key(
        config: &LocalSignerConfig,
        private_key: &str,
        chain_id: u64,
    ) -> Result<Self, SignerError> {
        let signer = PrivateKeySigner::from_str(private_key)
            .map_err(|_| SignerError::InvalidPrivateKey {
                env: config.private_key_env.clone(),
            })?
            .with_chain_id(Some(chain_id));
        let signer_address = signer.address();
        Ok(Self {
            signer,
            signer_address,
        })
    }

    /// Return the EVM signer address.
    pub fn signer_address(&self) -> Address {
        self.signer_address
    }

    /// Return the EVM signer address as a hex string.
    pub fn signer_address_string(&self) -> String {
        self.signer_address.to_string()
    }

    /// Borrow the Alloy local signer for later broadcast integration.
    pub fn signer(&self) -> &PrivateKeySigner {
        &self.signer
    }

    pub async fn broadcast(
        &self,
        rpc_url: &str,
        tx: TransactionRequest,
    ) -> Result<LocalPendingExecution, SignerError> {
        let parsed_url = Url::parse(rpc_url).map_err(|_| SignerError::BroadcastFailed)?;
        let provider = ProviderBuilder::new().wallet(self.signer.clone()).connect_http(parsed_url);
        let pending = provider
            .send_transaction(tx)
            .await
            .map_err(|_| SignerError::BroadcastFailed)?;
        let tx_hash = *pending.tx_hash();
        Ok(LocalPendingExecution { tx_hash, pending })
    }

    pub async fn wait_for_receipt(
        &self,
        pending: LocalPendingExecution,
        receipt_timeout: Duration,
    ) -> Result<LocalExecutionReceipt, SignerError> {
        let tx_hash = pending.tx_hash;
        let receipt = pending
            .pending
            .with_timeout(Some(receipt_timeout))
            .get_receipt()
            .await
            .map_err(|err| {
                if err.to_string().to_ascii_lowercase().contains("timeout") {
                    SignerError::ReceiptTimeout {
                        tx_hash: tx_hash.to_string(),
                    }
                } else {
                    SignerError::ReceiptFailed
                }
            })?;
        let receipt_status = if receipt.status() {
            LocalReceiptStatus::Success
        } else {
            LocalReceiptStatus::Reverted
        };
        Ok(LocalExecutionReceipt {
            tx_hash,
            receipt_status,
            gas_used: receipt.gas_used().to_string(),
        })
    }
}

impl std::fmt::Debug for LocalSignerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalSignerHandle")
            .field("signer_address", &self.signer_address)
            .finish_non_exhaustive()
    }
}
