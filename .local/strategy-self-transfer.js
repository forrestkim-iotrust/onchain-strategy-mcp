// Base mainnet first dogfood: native self-transfer 0.00001 ETH to prove pipeline.
((BURNER) => (ctx) => [
    ctx.actions.nativeTransfer({
        to: BURNER,
        value: "10000000000000", // 0.00001 ETH in wei
    }),
])("0xe32f0F034C544040D147F7094F223a9C61CDf23F");
