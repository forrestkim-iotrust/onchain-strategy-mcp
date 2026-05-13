// Auto-funnel example: when ETH or USDC arrives at BURNER, swap excess ETH
// for USDC on Uniswap V3 (keeping a gas reserve) and supply USDC to Aave V3.
//
// Register with strategy_register, then attach two `log` triggers:
//   1. NativeReceived event at BURNER (requires EIP-7702 BatchExec v2+)
//   2. USDC.Transfer event where topic2 = padded(BURNER)
//
// See README.md "Use case 1" for the full setup.
((BURNER, WETH, USDC, ROUTER, AAVE, RESERVE_WEI, MIN_SWAP_WEI, MIN_DEPOSIT_USDC) => (ctx) => {
    const eth = BigInt(ctx.evm.nativeBalance(BURNER, "pending"));
    const usdc = BigInt(ctx.evm.erc20Balance(USDC, BURNER, "pending"));
    const reserve = BigInt(RESERVE_WEI);
    const minSwap = BigInt(MIN_SWAP_WEI);
    const minDeposit = BigInt(MIN_DEPOSIT_USDC);

    if (eth > reserve + minSwap) {
        const excess = eth - reserve;
        ctx.log("ETH->USDC swap " + excess.toString());
        return [
            ctx.actions.contractCall({
                address: ROUTER,
                abi: [{
                    name: "exactInputSingle", type: "function", stateMutability: "payable",
                    inputs: [{ name: "params", type: "tuple", components: [
                        { name: "tokenIn",          type: "address" },
                        { name: "tokenOut",         type: "address" },
                        { name: "fee",              type: "uint24"  },
                        { name: "recipient",        type: "address" },
                        { name: "amountIn",         type: "uint256" },
                        { name: "amountOutMinimum", type: "uint256" },
                        { name: "sqrtPriceLimitX96",type: "uint160" }
                    ]}],
                    outputs: [{ name: "amountOut", type: "uint256" }]
                }],
                function: "exactInputSingle",
                args: [[WETH, USDC, "500", BURNER, excess.toString(), "0", "0"]],
                value: excess.toString()
            })
        ];
    }

    if (usdc >= minDeposit) {
        ctx.log("USDC->Aave deposit " + usdc.toString());
        return [
            ctx.actions.erc20Approve({ token: USDC, spender: AAVE, amount: usdc.toString() }),
            ctx.actions.contractCall({
                address: AAVE,
                abi: [{
                    name: "supply", type: "function", stateMutability: "nonpayable",
                    inputs: [
                        { name: "asset",        type: "address" },
                        { name: "amount",       type: "uint256" },
                        { name: "onBehalfOf",   type: "address" },
                        { name: "referralCode", type: "uint16"  }
                    ],
                    outputs: []
                }],
                function: "supply",
                args: [USDC, usdc.toString(), BURNER, "0"]
            })
        ];
    }

    ctx.log("noop eth=" + eth.toString() + " usdc=" + usdc.toString());
    return "noop";
})(
    "0xYourBurnerAddress",                        // BURNER
    "0x4200000000000000000000000000000000000006", // WETH on Base
    "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913", // USDC on Base
    "0x2626664c2603336E57B271c5C0b26F421741e481", // Uniswap V3 SwapRouter02 on Base
    "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5", // Aave V3 Pool on Base
    "50000000000000",     // RESERVE: 0.00005 ETH gas reserve
    "10000000000000",     // MIN_SWAP: skip below 0.00001 ETH excess
    "10000"               // MIN_DEPOSIT: 0.01 USDC
)
