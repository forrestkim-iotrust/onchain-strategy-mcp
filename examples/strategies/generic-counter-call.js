// Local Anvil generic ABI contract call example.
// Replace COUNTER_ADDRESS with a counter contract deployed on chain 31337.

((COUNTER_ADDRESS, COUNTER_ABI) => (ctx) => [
    ctx.actions.contractCall({
        address: COUNTER_ADDRESS,
        abi: JSON.stringify(COUNTER_ABI),
        function: "increment",
        args: [],
    }),
])(
    "0x0000000000000000000000000000000000000003",
    [{
        type: "function",
        name: "increment",
        inputs: [],
        outputs: [],
        stateMutability: "nonpayable",
    }],
)
