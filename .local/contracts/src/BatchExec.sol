// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

// EIP-7702 delegate target. After authorization, this code runs at the EOA
// address. The only authorized caller is the EOA itself — i.e. msg.sender
// must equal address(this), which is the delegated EOA address.
contract BatchExec {
    struct Call {
        address to;
        uint256 value;
        bytes data;
    }

    error OnlySelf();
    error CallFailed(uint256 index, bytes returndata);

    function executeBatch(Call[] calldata calls) external payable {
        if (msg.sender != address(this)) revert OnlySelf();
        uint256 n = calls.length;
        for (uint256 i = 0; i < n; ++i) {
            (bool ok, bytes memory ret) = calls[i].to.call{value: calls[i].value}(calls[i].data);
            if (!ok) revert CallFailed(i, ret);
        }
    }
}
