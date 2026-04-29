// Local Anvil ERC20 approve example.
// Replace TOKEN_ADDRESS and SPENDER_ADDRESS with addresses deployed on chain 31337.

const TOKEN_ADDRESS = "0x0000000000000000000000000000000000000001";
const SPENDER_ADDRESS = "0x0000000000000000000000000000000000000002";
const APPROVE_AMOUNT = "1000000000000000000";

(ctx) => [
    ctx.actions.erc20Approve({
        token: TOKEN_ADDRESS,
        spender: SPENDER_ADDRESS,
        amount: APPROVE_AMOUNT,
    }),
]
