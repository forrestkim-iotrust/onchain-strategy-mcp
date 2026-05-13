// Base mainnet Aave V3 USDC deposit — first MCP-driven mainnet dogfood.
// Burner approves Aave Pool for 0.1 USDC, then supplies 0.1 USDC on its own behalf.
((USDC, AAVE_POOL, BURNER, AMOUNT) => (ctx) => [
    ctx.actions.erc20Approve({
        token: USDC,
        spender: AAVE_POOL,
        amount: AMOUNT,
    }),
    ctx.actions.contractCall({
        address: AAVE_POOL,
        abi: [{
            name: "supply",
            type: "function",
            stateMutability: "nonpayable",
            inputs: [
                { name: "asset", type: "address" },
                { name: "amount", type: "uint256" },
                { name: "onBehalfOf", type: "address" },
                { name: "referralCode", type: "uint16" },
            ],
            outputs: [],
        }],
        function: "supply",
        args: [USDC, AMOUNT, BURNER, "0"],
    }),
])(
    "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913", // USDC (Base)
    "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5", // Aave V3 Pool (Base)
    "0xe32f0F034C544040D147F7094F223a9C61CDf23F", // burner
    "100000", // 0.1 USDC (6 decimals)
);
