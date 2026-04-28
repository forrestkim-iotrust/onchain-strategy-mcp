//! Local Alloy private-key signer boundary.

use std::str::FromStr;

use alloy::signers::{Signer as AlloySigner, local::PrivateKeySigner};
use alloy_primitives::Address;

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
}

impl std::fmt::Debug for LocalSignerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalSignerHandle")
            .field("signer_address", &self.signer_address)
            .finish_non_exhaustive()
    }
}
