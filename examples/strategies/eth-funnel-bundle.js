// eth-funnel-bundle.js — v1.4 strategy bundle (execute + records + view).
//
// Same behavior as eth-funnel.js, but ships the records schema and view
// function so `strategy://{id}/view` returns principal / accrued interest /
// activity counts without the agent writing ad-hoc evm_view JS.
//
// Register via strategy_register with all three sections; runtime captures
// records at action-confirm time and runs `view(ctx, records)` whenever the
// view resource is read.
//
// Constants
const BURNER = "0xe32f0F034C544040D147F7094F223a9C61CDf23F";
const WETH   = "0x4200000000000000000000000000000000000006";
const USDC   = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
const AUSDC  = "0x4e65fE4DbA92790696d040ac24Aa414708F5c0AB";
const ROUTER = "0x2626664c2603336E57B271c5C0b26F421741e481"; // Uniswap V3 SwapRouter02
const AAVE   = "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5"; // Aave V3 Pool

const UNISWAP_V3_ROUTER_ABI = [{
  name: "exactInputSingle", type: "function", stateMutability: "payable",
  inputs: [{
    name: "params", type: "tuple", components: [
      { name: "tokenIn",          type: "address" },
      { name: "tokenOut",         type: "address" },
      { name: "fee",              type: "uint24"  },
      { name: "recipient",        type: "address" },
      { name: "amountIn",         type: "uint256" },
      { name: "amountOutMinimum", type: "uint256" },
      { name: "sqrtPriceLimitX96",type: "uint160" }
    ]
  }],
  outputs: [{ name: "amountOut", type: "uint256" }]
}];

const AAVE_POOL_ABI = [{
  name: "supply", type: "function", stateMutability: "nonpayable",
  inputs: [
    { name: "asset",       type: "address" },
    { name: "amount",      type: "uint256" },
    { name: "onBehalfOf",  type: "address" },
    { name: "referralCode",type: "uint16"  }
  ],
  outputs: []
}];


// ─────────────────── 1. execute ───────────────────
// Same shape as a v1.3 strategy. Returns Action[] or "noop".
const execute = (ctx) => {
  const eth  = BigInt(ctx.evm.nativeBalance(BURNER, "pending"));
  const usdc = BigInt(ctx.evm.erc20Balance(USDC, BURNER, "pending"));

  const RESERVE     = 50_000_000_000_000n;  // 0.00005 ETH kept for gas
  const MIN_SWAP    = 10_000_000_000_000n;  // 0.00001 ETH min above reserve
  const MIN_DEPOSIT = 10_000n;              // 0.01 USDC

  if (eth > RESERVE + MIN_SWAP) {
    const excess = eth - RESERVE;
    return [
      ctx.actions.contractCall({
        address: ROUTER,
        abi: UNISWAP_V3_ROUTER_ABI,
        function: "exactInputSingle",
        args: [[WETH, USDC, "500", BURNER, excess.toString(), "0", "0"]],
        value: excess.toString(),
      }),
    ];
  }

  if (usdc >= MIN_DEPOSIT) {
    return [
      ctx.actions.erc20Approve({ token: USDC, spender: AAVE, amount: usdc.toString() }),
      ctx.actions.contractCall({
        address: AAVE,
        abi: AAVE_POOL_ABI,
        function: "supply",
        args: [USDC, usdc.toString(), BURNER, "0"],
      }),
    ];
  }

  return "noop";
};


// ─────────────────── 2. records ───────────────────
// Declarative capture schema. Runtime watches confirmed actions and writes
// matching captures into strategy_records_capture. The view function reads
// them back as aggregate handles ({ sum(field), count, latest, since(ts), each }).
const records = [
  {
    name: "supply",
    on: {
      kind: "contractCall",
      target: AAVE,
      selector: "supply",
    },
    capture: {
      amount_micro:    "args[1]",
      asset:           "args[0]",
      block:           "tx.block",
      ts:              "tx.ts",
      tx_hash:         "tx.hash",
      // Snapshot the liquidityIndex at deposit time so accrued interest
      // is computable without an archive RPC later.
      index_at_block:  "view.aaveLiquidityIndex(args[0])",
    },
  },
  {
    name: "swap",
    on: {
      kind: "contractCall",
      target: ROUTER,
      selector: "exactInputSingle",
    },
    capture: {
      eth_in_wei:     "args[0][4]",
      // `logs.Transfer[self].value` — first ERC20 Transfer with `to == burner`
      // inside this tx. For a swap, that's the USDC the pool minted to us.
      usdc_out_micro: "logs.Transfer[self].value",
      ts:             "tx.ts",
      tx_hash:        "tx.hash",
    },
  },
];


// ─────────────────── 3. view ───────────────────
// Read by `strategy://{id}/view`. Receives current ctx + the captured records.
// MUST be pure-read (no actions). Same sandbox as evm_view.
//
// The top-level `$assets` array (see `docs://strategy-bundle`) declares the
// strategy's user-held positions. The runtime aggregates `$assets` across all
// active strategies for the portfolio total. Everything outside `$assets` is
// per-strategy observation — rendered on the strategy card, not summed.
const view = (ctx, records) => {
  const principal_micro = records.supply.sum("amount_micro");
  const current_micro   = BigInt(ctx.evm.erc20Balance(AUSDC, BURNER, "pending"));
  const accrued_micro   = current_micro - BigInt(principal_micro);

  return {
    // ─── user positions (portfolio aggregate) ───
    $assets: [
      {
        chain_id: 8453,
        venue:    "aave-v3-base",
        asset:    "USDC",
        address:  AUSDC,                              // aToken contract, ERC20
        amount:   (Number(current_micro) / 1e6).toFixed(6),
        raw:      current_micro.toString(),
        decimals: 6,
        usd:      Number(current_micro) / 1e6        // 1:1 stable, no oracle needed
      }
    ],

    // ─── per-strategy observation (not aggregated) ───
    earnings: {
      principal_usdc:        Number(principal_micro) / 1e6,
      accrued_interest_usdc: Number(accrued_micro)   / 1e6,
    },
    activity: {
      supply_count:      records.supply.count,
      swap_count:        records.swap.count,
      total_eth_swapped: Number(records.swap.sum("eth_in_wei"))     / 1e18,
      total_usdc_minted: Number(records.swap.sum("usdc_out_micro")) / 1e6,
      last_supply_ts:    records.supply.latest?.ts,
      last_swap_ts:      records.swap.latest?.ts,
    },
  };
};


// Module exports are illustrative — the runtime takes each piece as a
// separate argument in strategy_register:
//   strategy_register({
//     name: "eth-funnel-bundle-v1",
//     source: <execute as string>,
//     records: <records array>,
//     view:   <view function as string>,
//   })
module.exports = { execute, records, view };
