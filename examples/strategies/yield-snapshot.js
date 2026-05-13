// Yield observer: read USDC supply APYs from Aave V3, Compound III, and
// Moonwell on Base. Returns "noop" — logs the snapshot to journal_logs so
// running it on an `interval` trigger accumulates a time series for free.
//
// Pair with a historical backfill view (see README.md "Use case 3") to get
// past data via `blockTag` archive reads.
((USDC, AAVE, COMET, MWELL) => (ctx) => {
    const RAY = 1000000000000000000000000000n;
    const E18 = 1000000000000000000n;
    const SEC = 31536000n;
    const snap = { aave: "ERR", comet: "ERR", moonwell: "ERR" };

    try {
        const r = ctx.evm.readContract({
            address: AAVE,
            abi: [{ type: "function", name: "getReserveData", stateMutability: "view",
                inputs: [{ name: "a", type: "address" }],
                outputs: [{ name: "d", type: "tuple", components: [
                    { name: "c",  type: "uint256" }, { name: "l",  type: "uint128" },
                    { name: "r",  type: "uint128" }, { name: "vi", type: "uint128" },
                    { name: "vr", type: "uint128" }, { name: "sr", type: "uint128" },
                    { name: "t",  type: "uint40"  }, { name: "id", type: "uint16"  },
                    { name: "a1", type: "address" }, { name: "a2", type: "address" },
                    { name: "a3", type: "address" }, { name: "a4", type: "address" },
                    { name: "u1", type: "uint128" }, { name: "u2", type: "uint128" },
                    { name: "u3", type: "uint128" }
                ]}]
            }],
            function: "getReserveData",
            args: [USDC]
        });
        // currentLiquidityRate is APR in RAY (1e27) units.
        snap.aave = ((BigInt(r[2]) * 10000n) / RAY).toString();
    } catch (e) { snap.aave = "ERR:" + e.message; }

    try {
        const C = [
            { type: "function", name: "getUtilization", stateMutability: "view",
              inputs: [], outputs: [{ name: "u", type: "uint256" }] },
            { type: "function", name: "getSupplyRate", stateMutability: "view",
              inputs: [{ name: "u", type: "uint256" }], outputs: [{ name: "r", type: "uint64" }] }
        ];
        const util = ctx.evm.readContract({ address: COMET, abi: C, function: "getUtilization", args: [] });
        const rate = ctx.evm.readContract({ address: COMET, abi: C, function: "getSupplyRate", args: [util] });
        // per-second rate scaled by 1e18 → annualized bps.
        snap.comet = ((BigInt(rate) * SEC * 10000n) / E18).toString();
    } catch (e) { snap.comet = "ERR:" + e.message; }

    try {
        const M = [{ type: "function", name: "supplyRatePerTimestamp", stateMutability: "view",
                     inputs: [], outputs: [{ name: "r", type: "uint256" }] }];
        const rate = ctx.evm.readContract({ address: MWELL, abi: M, function: "supplyRatePerTimestamp", args: [] });
        snap.moonwell = ((BigInt(rate) * SEC * 10000n) / E18).toString();
    } catch (e) { snap.moonwell = "ERR:" + e.message; }

    ctx.log("YIELD_SNAPSHOT " + JSON.stringify(snap));
    return "noop";
})(
    "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913", // USDC (Base)
    "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5", // Aave V3 Pool (Base)
    "0xb125E6687d4313864e53df431d5425969c15Eb2F", // Compound III cUSDCv3 (Base)
    "0xEdc817A28E8B93B03976FBd4a3dDBc9f7D176c22"  // Moonwell mUSDC (Base)
)
