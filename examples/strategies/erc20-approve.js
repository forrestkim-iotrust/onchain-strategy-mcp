// Local Anvil ERC20 approve-shaped example.
// Replace TOKEN_ADDRESS and SPENDER_ADDRESS with a local token or test contract
// that accepts approve(address,uint256) on chain 31337.

((TOKEN_ADDRESS, SPENDER_ADDRESS, APPROVE_AMOUNT) => (ctx) => [
    ctx.actions.erc20Approve({
        token: TOKEN_ADDRESS,
        spender: SPENDER_ADDRESS,
        amount: APPROVE_AMOUNT,
    }),
])(
    "0x0000000000000000000000000000000000000001",
    "0x0000000000000000000000000000000000000002",
    "0",
)
