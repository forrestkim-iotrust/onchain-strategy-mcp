// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

/// @title BatchExec — minimal EIP-7702 delegate target
/// @notice After authorization, this contract's code runs AT the EOA's
///   address. The EOA can then batch multiple calls into a single tx where
///   `msg.sender == EOA` for every inner call.
/// @dev Deploy once per chain. Pass the deployed address as `[aa].delegate`
///   in `.local/config.toml`. The runtime's signer generates the EIP-7702
///   authorization automatically when a strategy returns ≥2 actions AND
///   the delegate is configured.
contract BatchExec {
    struct Call {
        address to;
        uint256 value;
        bytes data;
    }

    error OnlySelf();
    error CallFailed(uint256 index, bytes returndata);

    /// Emitted by `receive()` on plain ETH transfers TO a 7702-delegated EOA.
    /// Lets `log`-kind triggers detect incoming ETH (which produces no log
    /// otherwise — and Base's centralized sequencer makes mempool-based
    /// detection unreliable).
    event NativeReceived(address indexed from, uint256 amount);

    function executeBatch(Call[] calldata calls) external payable {
        if (msg.sender != address(this)) revert OnlySelf();
        uint256 n = calls.length;
        for (uint256 i = 0; i < n; ++i) {
            (bool ok, bytes memory ret) = calls[i].to.call{value: calls[i].value}(calls[i].data);
            if (!ok) revert CallFailed(i, ret);
        }
    }

    /// Required so plain ETH transfers to the delegated EOA succeed AND
    /// emit a detectable event.
    receive() external payable {
        emit NativeReceived(msg.sender, msg.value);
    }
}
