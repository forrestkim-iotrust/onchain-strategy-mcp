//! v1.3: deterministic CREATE2 deployment for the BatchExec EIP-7702
//! delegate target.
//!
//! Every install on a given chain shares ONE BatchExec address derived via
//! the canonical Arachnid CREATE2 deployer (`0x4e59…956C`) — the first user
//! pays the gas, everyone else sees the contract already present at the
//! predicted address. No `cast` / `forge` required at install time.
//!
//! Inputs to the address derivation are immutable constants:
//!   - [`BATCH_EXEC_INIT_CODE`]   compiled bytecode of `BatchExec.sol`
//!   - [`BATCH_EXEC_INIT_CODE_HASH`]  `keccak256(init_code)` (asserted at test time)
//!   - [`DEPLOY_SALT`]            `keccak256("onchain-strategy-mcp:BatchExec:v1")`
//!   - [`ARACHNID_DEPLOYER`]      canonical CREATE2 deployer
//!
//! Editing the contract source bumps `BATCH_EXEC_INIT_CODE`, which changes
//! the predicted address — the determinism test in this module ensures the
//! drift is caught at `cargo test` time rather than at deploy time.

use alloy_primitives::{Address, B256, address, hex, keccak256};

/// Compiled creation bytecode (constructor + runtime) for
/// `examples/contracts/BatchExec.sol`, produced by Solidity 0.8.26 with
/// default forge build settings. Recompile and regenerate via:
///
/// ```sh
/// forge build --use 0.8.26
/// jq -r '.bytecode.object' out/BatchExec.sol/BatchExec.json
/// ```
pub const BATCH_EXEC_INIT_CODE: &[u8] = &hex!(
    "6080604052348015600e575f80fd5b506105b48061001c5f395ff3fe608060405260043610610021575f3560e01c806334fcd5be1461007a57610076565b36610076573373ffffffffffffffffffffffffffffffffffffffff167f8ac633e5b094e1150d2a6495df4d0c77f51d293abe99e7733c78870dfbee76603460405161006c9190610278565b60405180910390a2005b5f80fd5b610094600480360381019061008f91906102fa565b610096565b005b3073ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff16146100fb576040517f14d4a4e800000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f8282905090505f5b8181101561025a575f8085858481811061012157610120610345565b5b9050602002810190610133919061037e565b5f01602081019061014491906103ff565b73ffffffffffffffffffffffffffffffffffffffff1686868581811061016d5761016c610345565b5b905060200281019061017f919061037e565b6020013587878681811061019657610195610345565b5b90506020028101906101a8919061037e565b80604001906101b7919061042a565b6040516101c59291906104c8565b5f6040518083038185875af1925050503d805f81146101ff576040519150601f19603f3d011682016040523d82523d5f602084013e610204565b606091505b50915091508161024d5782816040517f5c0dee5d000000000000000000000000000000000000000000000000000000008152600401610244929190610550565b60405180910390fd5b5050806001019050610104565b50505050565b5f819050919050565b61027281610260565b82525050565b5f60208201905061028b5f830184610269565b92915050565b5f80fd5b5f80fd5b5f80fd5b5f80fd5b5f80fd5b5f8083601f8401126102ba576102b9610299565b5b8235905067ffffffffffffffff8111156102d7576102d661029d565b5b6020830191508360208202830111156102f3576102f26102a1565b5b9250929050565b5f80602083850312156103105761030f610291565b5b5f83013567ffffffffffffffff81111561032d5761032c610295565b5b610339858286016102a5565b92509250509250929050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52603260045260245ffd5b5f80fd5b5f80fd5b5f80fd5b5f8235600160600383360303811261039957610398610372565b5b80830191505092915050565b5f73ffffffffffffffffffffffffffffffffffffffff82169050919050565b5f6103ce826103a5565b9050919050565b6103de816103c4565b81146103e8575f80fd5b50565b5f813590506103f9816103d5565b92915050565b5f6020828403121561041457610413610291565b5b5f610421848285016103eb565b91505092915050565b5f808335600160200384360303811261044657610445610372565b5b80840192508235915067ffffffffffffffff82111561046857610467610376565b5b6020830192506001820236038313156104845761048361037a565b5b509250929050565b5f81905092915050565b828183375f83830152505050565b5f6104af838561048c565b93506104bc838584610496565b82840190509392505050565b5f6104d48284866104a4565b91508190509392505050565b5f81519050919050565b5f82825260208201905092915050565b8281835e5f83830152505050565b5f601f19601f8301169050919050565b5f610522826104e0565b61052c81856104ea565b935061053c8185602086016104fa565b61054581610508565b840191505092915050565b5f6040820190506105635f830185610269565b81810360208301526105758184610518565b9050939250505056fea26469706673582212208833c08cb00d4f2873cd4a37628dbb7ba6040e5512bea6c82f0258c3a50d50a064736f6c634300081a0033"
);

/// `keccak256(BATCH_EXEC_INIT_CODE)`. Hardcoded so an accidental edit to
/// the bytecode constant fails the determinism test below loudly.
pub const BATCH_EXEC_INIT_CODE_HASH: B256 = B256::new(hex!(
    "29f69ee9847aa26575dc986310ab7846087e0d5408d90389f6aaf5a99b2a16f8"
));

/// `keccak256("onchain-strategy-mcp:BatchExec:v1")`.
///
/// Pre-image is kept here for auditability; the hash itself is hardcoded
/// so a typo in the pre-image can't silently change the predicted address.
pub const DEPLOY_SALT_PREIMAGE: &str = "onchain-strategy-mcp:BatchExec:v1";
pub const DEPLOY_SALT: [u8; 32] = hex!(
    "9853ff86b2b919a920eff9e9a240fb83f930ad1a03b9025f2849d57ce340e2ac"
);

/// Canonical Arachnid CREATE2 deployer (Nick-method installed; present on
/// every major EVM chain). See <https://github.com/Arachnid/deterministic-deployment-proxy>.
pub const ARACHNID_DEPLOYER: Address = address!("4e59b44847b379578588920cA78FbF26c0B4956C");

/// CREATE2 address: `keccak256(0xff ‖ deployer ‖ salt ‖ keccak256(init_code))[12:]`.
pub fn predicted_delegate_address() -> Address {
    let mut buf = [0u8; 1 + 20 + 32 + 32];
    buf[0] = 0xff;
    buf[1..21].copy_from_slice(ARACHNID_DEPLOYER.as_slice());
    buf[21..53].copy_from_slice(&DEPLOY_SALT);
    buf[53..85].copy_from_slice(BATCH_EXEC_INIT_CODE_HASH.as_slice());
    Address::from_slice(&keccak256(buf)[12..])
}

/// Calldata for the Arachnid deployer: `salt ‖ init_code`.
pub fn deploy_calldata() -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + BATCH_EXEC_INIT_CODE.len());
    out.extend_from_slice(&DEPLOY_SALT);
    out.extend_from_slice(BATCH_EXEC_INIT_CODE);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_code_hash_matches_constant() {
        // If anyone edits BATCH_EXEC_INIT_CODE without updating
        // BATCH_EXEC_INIT_CODE_HASH, this test fails and stops the build.
        let actual = keccak256(BATCH_EXEC_INIT_CODE);
        assert_eq!(
            actual, BATCH_EXEC_INIT_CODE_HASH,
            "BATCH_EXEC_INIT_CODE changed without updating BATCH_EXEC_INIT_CODE_HASH"
        );
    }

    #[test]
    fn deploy_salt_matches_preimage() {
        let actual = keccak256(DEPLOY_SALT_PREIMAGE.as_bytes());
        assert_eq!(
            actual.as_slice(),
            &DEPLOY_SALT,
            "DEPLOY_SALT no longer matches DEPLOY_SALT_PREIMAGE"
        );
    }

    #[test]
    fn predicted_address_is_deterministic() {
        // Computed off-line once with the script in delegate.rs's doc
        // comment. Any drift here means EITHER the init code, the salt,
        // OR the deployer constant changed — and the install/deploy
        // story changes with it. Fail loudly.
        let expected = address!("821fd81668823a3c5a65e95ced5f050ee54a4f53");
        assert_eq!(predicted_delegate_address(), expected);
    }

    #[test]
    fn deploy_calldata_layout() {
        let cd = deploy_calldata();
        assert_eq!(&cd[..32], &DEPLOY_SALT);
        assert_eq!(&cd[32..], BATCH_EXEC_INIT_CODE);
    }
}
