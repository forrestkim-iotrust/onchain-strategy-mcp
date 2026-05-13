//! v1.3: OS keychain–backed signer resolution.
//!
//! Keys are stored in the platform-native secret store under
//! `service = "onchain-strategy-mcp"` and `account = <key_id>` (default
//! `"default"`). The private key value is hex (no `0x` prefix).
//!
//! This module never logs key material and never writes it to disk.

use std::str::FromStr;

use alloy::signers::{Signer as AlloySigner, local::PrivateKeySigner};

use crate::{LocalSignerConfig, LocalSignerHandle, SignerError};

/// Stable keychain service name (do not change — naming contract).
pub const KEYCHAIN_SERVICE: &str = "onchain-strategy-mcp";

/// Load a [`LocalSignerHandle`] by reading the hex private key from the OS
/// keychain. The key id is taken from `config.key_id`.
pub fn load_from_keychain(
    config: &LocalSignerConfig,
    chain_id: u64,
) -> Result<LocalSignerHandle, SignerError> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &config.key_id).map_err(|err| {
        SignerError::KeychainBackend {
            detail: err.to_string(),
        }
    })?;
    let raw = match entry.get_password() {
        Ok(v) => v,
        Err(keyring::Error::NoEntry) => {
            return Err(SignerError::KeychainNotFound {
                service: KEYCHAIN_SERVICE.to_string(),
                key_id: config.key_id.clone(),
            });
        }
        Err(err) => {
            return Err(SignerError::KeychainBackend {
                detail: err.to_string(),
            });
        }
    };
    let signer = PrivateKeySigner::from_str(raw.trim())
        .map_err(|_| SignerError::InvalidPrivateKey {
            env: format!("keychain:{}", config.key_id),
        })?
        .with_chain_id(Some(chain_id));
    Ok(LocalSignerHandle::__from_alloy_signer(signer))
}

/// Generate a fresh secp256k1 keypair. Returns `(hex_private_key, eip55_address)`.
///
/// The hex string has NO `0x` prefix (per the v1.3 storage contract).
pub fn generate_burner() -> (String, String) {
    let signer = PrivateKeySigner::random();
    let key_bytes = signer.to_bytes();
    let hex_key = hex_encode(key_bytes.as_slice());
    let addr = signer.address().to_string();
    (hex_key, addr)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Store a hex private key (no `0x` prefix) in the OS keychain.
///
/// Used by `executor-mcp init`. Returns `KeychainBackend` on any
/// underlying error (locked, libsecret missing, etc.).
pub fn store_in_keychain(key_id: &str, hex_private_key: &str) -> Result<(), SignerError> {
    let entry =
        keyring::Entry::new(KEYCHAIN_SERVICE, key_id).map_err(|err| SignerError::KeychainBackend {
            detail: err.to_string(),
        })?;
    entry
        .set_password(hex_private_key)
        .map_err(|err| SignerError::KeychainBackend {
            detail: err.to_string(),
        })
}
