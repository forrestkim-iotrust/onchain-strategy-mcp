//! `AnvilFixture` — Phase 4 D-14.
//!
//! Skip-cleanly contract:
//! - If `ANVIL_RPC_URL` is set, use that URL (no spawn). Tests that depend
//!   on anvil-pre-funded accounts skip in this mode (`funded_accounts` empty).
//! - Otherwise call `Anvil::new().chain_id(31337).try_spawn()`. On failure
//!   (binary missing), `eprintln!` a skip message and return `None`. Tests
//!   detect `None` and early-return — never panic.

use alloy::node_bindings::{Anvil, AnvilInstance};
use alloy_primitives::Address;
use url::Url;

#[allow(clippy::print_stderr)] // approved skip message per D-14.
pub struct AnvilFixture {
    /// `None` when the fixture used `ANVIL_RPC_URL` (external devnet).
    pub instance: Option<AnvilInstance>,
    pub rpc_url: Url,
    pub funded_accounts: Vec<Address>,
}

impl AnvilFixture {
    pub fn try_spawn() -> Option<Self> {
        if let Ok(url) = std::env::var("ANVIL_RPC_URL") {
            let rpc_url: Url = url.parse().ok()?;
            return Some(Self {
                instance: None,
                rpc_url,
                funded_accounts: vec![],
            });
        }
        match Anvil::new().chain_id(31337).try_spawn() {
            Ok(instance) => {
                let rpc_url = instance.endpoint_url();
                let funded = instance.addresses().to_vec();
                Some(Self {
                    instance: Some(instance),
                    rpc_url,
                    funded_accounts: funded,
                })
            }
            Err(_e) => {
                #[allow(clippy::print_stderr)]
                {
                    eprintln!(
                        "[skip] anvil binary not on PATH; install foundry to run anvil-tests"
                    );
                }
                None
            }
        }
    }
}
