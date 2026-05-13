//! Local Alloy private-key signer boundary.

use std::{str::FromStr, time::Duration};

use alloy::{
    eips::eip7702::Authorization,
    network::{Ethereum, ReceiptResponse, TransactionBuilder, TransactionBuilder7702},
    providers::{PendingTransactionBuilder, Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
    signers::{Signer as AlloySigner, local::PrivateKeySigner},
};
use alloy_primitives::{Address, B256, Bytes, U256};
use alloy_sol_types::{SolCall, sol};
use reqwest::Url;

use crate::{LocalSignerConfig, SignerError};

sol! {
    /// BatchExec.executeBatch — EIP-7702 delegate target signature.
    #[allow(missing_docs)]
    struct Eip7702Call { address to; uint256 value; bytes data; }
    #[allow(missing_docs)]
    function executeBatch(Eip7702Call[] calls);
}

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

    /// EIP-7702: bundle multiple calls into a single transaction.
    ///
    /// Signs a 7702 authorization delegating the burner EOA to `delegate`,
    /// then sends a single tx from burner to burner with calldata
    /// `executeBatch(calls)`. Inside the delegated execution `msg.sender`
    /// remains the burner address (delegation runs the delegate's code AT
    /// the burner's address).
    ///
    /// Returns the pending tx — caller uses `wait_for_receipt`.
    pub async fn send_7702_batch(
        &self,
        rpc_url: &str,
        delegate: Address,
        calls: Vec<(Address, U256, Bytes)>,
    ) -> Result<LocalPendingExecution, SignerError> {
        let parsed_url = Url::parse(rpc_url).map_err(|_| SignerError::BroadcastFailed)?;
        let provider = ProviderBuilder::new()
            .wallet(self.signer.clone())
            .connect_http(parsed_url);

        let chain_id = provider
            .get_chain_id()
            .await
            .map_err(|_| SignerError::BroadcastFailed)?;
        let nonce = provider
            .get_transaction_count(self.signer_address)
            .await
            .map_err(|_| SignerError::BroadcastFailed)?;

        // EIP-7702: when auth.signer == tx.signer, auth.nonce = tx.nonce + 1
        // because the EOA nonce increments via the tx first, then auth applies.
        let auth = Authorization {
            chain_id: U256::from(chain_id),
            address: delegate,
            nonce: nonce + 1,
        };
        let sig = self
            .signer
            .sign_hash(&auth.signature_hash())
            .await
            .map_err(|_| SignerError::BroadcastFailed)?;
        let signed_auth = auth.into_signed(sig);

        let encoded_calls: Vec<Eip7702Call> = calls
            .into_iter()
            .map(|(to, value, data)| Eip7702Call { to, value, data })
            .collect();
        let calldata = executeBatchCall {
            calls: encoded_calls,
        }
        .abi_encode();

        let tx = TransactionRequest::default()
            .with_to(self.signer_address)
            .with_input(calldata)
            .with_authorization_list(vec![signed_auth]);

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
