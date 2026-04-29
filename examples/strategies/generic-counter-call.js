// Local Anvil generic ABI contract call example.
// Replace COUNTER_ADDRESS with a counter contract deployed on chain 31337.

const COUNTER_ADDRESS = "0x0000000000000000000000000000000000000003";
const COUNTER_ABI = [
    {
        type: "function",
        name: "increment",
        inputs: [],
        outputs: [],
        stateMutability: "nonpayable",
    },
];

(ctx) => [
    ctx.actions.contractCall({
        address: COUNTER_ADDRESS,
        abi: JSON.stringify(COUNTER_ABI),
        function: "increment",
        args: [],
    }),
]
