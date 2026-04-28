---
phase: 04-evm-context-and-actions
reviewed: 2026-04-27T00:00:00Z
depth: standard
files_reviewed: 23
files_reviewed_list:
  - Cargo.toml
  - crates/executor-evm/Cargo.toml
  - crates/executor-evm/src/lib.rs
  - crates/executor-evm/src/error.rs
  - crates/executor-evm/src/config.rs
  - crates/executor-evm/src/provider.rs
  - crates/executor-evm/src/dyn_abi.rs
  - crates/executor-evm/src/read.rs
  - crates/executor-evm/src/erc20.rs
  - crates/executor-evm/src/native.rs
  - crates/executor-evm/src/action.rs
  - crates/executor-evm/src/units.rs
  - crates/executor-evm/src/address.rs
  - crates/executor-core/src/schema/action.rs
  - crates/executor-mcp/Cargo.toml
  - crates/executor-mcp/src/config.rs
  - crates/executor-mcp/src/errors.rs
  - crates/executor-mcp/src/server.rs
  - crates/executor-mcp/src/tools.rs
  - crates/executor-mcp/src/validation.rs
  - crates/executor-mcp/src/main.rs
  - crates/executor-state/src/schema.rs
  - crates/executor-state/src/journal.rs
  - crates/strategy-js/Cargo.toml
  - crates/strategy-js/src/lib.rs
  - crates/strategy-js/src/error.rs
  - crates/strategy-js/src/runtime.rs
  - crates/strategy-js/src/sandbox.rs
findings:
  blocker: 2
  warning: 8
  info: 3
  total: 13
status: issues_found
---

# Phase 4: Code Review Report

**Reviewed:** 2026-04-27
**Depth:** standard
**Files Reviewed:** 23 (excluding Cargo manifests & schema goldens; manifests inspected)
**Status:** issues_found

## Summary

Phase 4 lands the alloy-backed `executor-evm` crate, the `ctx.evm.*` /
`ctx.actions.*` / `ctx.units.*` / `ctx.address.*` host bindings on the
Phase-3 sandbox, the five-variant `Action` enum extension, and the journal
seq column on `journal_source_reads`. The work is largely well-structured
and the Phase-3 anti-patterns (HR-01, MR-01, MR-03, MR-04) are visibly
preserved. However, two BLOCKERs and several WARNINGs were found:

- **BR-01 (BLOCKER):** the entire D-12 EVM `data.kind` taxonomy
  (`evm_rpc_error` / `evm_decode_error` / `evm_revert`) is unreachable on
  the production wire path. EVM errors raised from inside the JS sandbox
  are converted to a JS-level `Error` whose only carrier back to Rust is
  the exception message — which surfaces as `RuntimeError::Exception`,
  classified as `data.kind == "exception"`. `RuntimeError::Evm(_)` is
  constructed only in unit tests. The wire test in `errors.rs` proves the
  mapping in isolation, but there is no production code path that ever
  yields `RuntimeError::Evm`.
- **BR-02 (BLOCKER):** the 64 KiB ABI cap (D-08 / RESEARCH Pitfall 11) is
  enforced only at builder time inside the JS sandbox
  (`ctx.actions.contractCall`). It is NOT enforced at the JSON-output gate
  (`validate_strategy_output`) and there is no `deserialize_with` on
  `ContractCallAction.abi`. A strategy that returns a hand-constructed
  `{kind:"contract_call", abi:"..." /* 1 MiB */}` array bypasses the cap.
  D-08 calls out both enforcement sites; only one is wired.

D-15 carry-forward observations:
- HR-01: PRESERVED. `FORBIDDEN_GLOBALS_SCRUB` runs before
  `c.globals().set("__ctx", ...)` (sandbox.rs:486-495); all Phase-4
  sub-objects are added to the local `ctx_obj` only and never installed
  on globalThis.
- MR-01: SUBSTANTIALLY PRESERVED. EvmError::Display is wire-safe;
  raw alloy/reqwest text routes via `tracing::warn!`. One soft leak
  exists in revert reason (see WR-04).
- MR-03: PRESERVED. `record_action` and `RuntimeContext::flush` both
  `?`-propagate serde failures through `StateError::SerializationError`.
- MR-04: PRESERVED. `journal_source_reads` gained `seq INTEGER NOT NULL` +
  `UNIQUE (run_id, seq)`; `next_source_read_seq` mirrors `next_log_seq`.

NOTE-1..NOTE-4 from plan-checker: NOTE-1 (zeroAddress reassignment) and
NOTE-2 (default blockTag = Latest) closed by 04-04 / 04-02 summaries
respectively. NOTE-3 (revert taxonomy) is documented but exposes BR-01:
even when `classify_provider_error` correctly produces `EvmError::Revert`,
the wire-side never sees the `evm_revert` kind because the error is
re-thrown as a JS exception. NOTE-4 (clock source) is a non-issue.

New Phase-4-specific risks not covered by Phase 3:
- BR-01 / BR-02 above.
- Revert-reason content is agent/contract controlled and reaches the wire
  unsanitized (WR-04).
- `block_in_place` invoked from inside `tokio::task::spawn_blocking`
  (WR-01) — works in current tests, but is documented Tokio API misuse.

## Blocker Issues

### BR-01: D-12 EVM `data.kind` taxonomy is unreachable on the production wire

**Files:**
- `crates/strategy-js/src/sandbox.rs:786-799, 1063-1075, 1170-1182,
  1240-1249` (host bindings throw EvmError as JS string)
- `crates/strategy-js/src/error.rs:37-43` (RuntimeError::Evm variant)
- `crates/executor-mcp/src/errors.rs:121, 131-148` (map_evm_error)

**Issue:** D-12 mandates extended `data.kind` taxonomy
(`evm_rpc_error` / `evm_decode_error` / `evm_revert`) on `-32017`. The
mapping `RuntimeError::Evm(EvmError) → map_evm_error → typed kind`
exists, but the `Evm` variant is NEVER constructed in production:

```
$ grep -rn "RuntimeError::Evm" crates/
crates/executor-mcp/src/errors.rs:121: ... // dispatch arm
crates/executor-mcp/src/errors.rs:131: ... // function decl
crates/executor-mcp/src/errors.rs:396,412,427,441,448  // unit tests only
```

In every host binding (read_contract, erc20, native_balance,
native_block_number, builders), an `EvmError` is converted to a JS
exception via `throw_js_error(&ctx, &stable)` where `stable` is
`EvmError::Display` (e.g. `"evm rpc error: transport"`). When QuickJS
unwinds this back to Rust, `caught_to_runtime_error` produces
`RuntimeError::Exception(stable_text)`. The MCP boundary then maps it to
`data.kind = "exception"`, NOT `"evm_rpc_error"` etc.

**Effect:** The agent dispatch contract documented in D-12 / 04-CONTEXT
("agents key off `data.kind` to dispatch on EVM failure mode") does not
hold for any actual EVM error. Every EVM failure surfaces as
`data.kind="exception"` with `data.detail` being the wire-safe Display
string. The taxonomy upgrade is decorative.

**Fix (sketch):** classify the JS exception message at the MCP boundary
and synthesize a `RuntimeError::Evm` before mapping. For example,
`classify_message` (sandbox.rs:642) could match the stable EvmError
prefixes (`"evm rpc error: "`, `"evm decode error: "`, `"evm revert: "`)
and reconstruct an `EvmError` variant for `map_runtime_error` to
dispatch:

```rust
fn classify_message(msg: &str) -> Option<RuntimeError> {
    // existing oom / stack_overflow / interrupted heuristics...
    if let Some(rest) = msg.strip_prefix("evm rpc error: ") {
        if rest == "timeout" {
            return Some(RuntimeError::Evm(EvmError::Timeout));
        }
        if rest == "transport" {
            return Some(RuntimeError::Evm(EvmError::Transport {
                detail_for_log: "<re-thrown from JS>".into(),
            }));
        }
    }
    if let Some(reason) = msg.strip_prefix("evm revert: ") {
        return Some(RuntimeError::Evm(EvmError::Revert {
            reason: reason.into(),
            detail_for_log: "<re-thrown from JS>".into(),
        }));
    }
    if let Some(category) = msg.strip_prefix("evm decode error: ") {
        // category is &str at runtime — can't re-attach &'static str,
        // but the kind dispatch only needs `Decode`.
        return Some(RuntimeError::Evm(EvmError::Decode {
            category: "rethrown",
            detail_for_log: category.into(),
        }));
    }
    None
}
```

Add a stdio integration test that asserts a real EVM failure produces
`data.kind == "evm_rpc_error" | "evm_decode_error" | "evm_revert"` — not
`"exception"`.

### BR-02: ABI 64 KiB cap not enforced at JSON-output gate

**Files:**
- `crates/executor-core/src/schema/action.rs:55-64` (ContractCallAction —
  no `deserialize_with` on `abi`)
- `crates/executor-mcp/src/tools.rs:430-467` (validate_strategy_output —
  does not call validate_abi_size or dry_run_abi_encode)
- `crates/executor-mcp/src/validation.rs:79-95`
  (validate_action_kind_allowlisted is the only per-action check)

**Issue:** D-08 specifies the cap MUST be enforced at BOTH builder time
AND serde-deserialization time:

> "Builder enforces at construct time; serde `deserialize_with`
>  enforces at validate-strategy-output time."

The builder enforcement is wired (sandbox.rs:1549 calls
`dry_run_abi_encode` which calls `validate_abi_size`). The
serde-deserialization enforcement is missing entirely:

- `ContractCallAction.abi` is a plain `pub abi: String` with no
  `deserialize_with`.
- `validate_strategy_output` (`tools.rs:430`) only checks that
  `kind` is allowlisted — it never inspects `abi.len()`.

A strategy that builds the JSON directly bypasses the cap:

```js
return [{ kind: "contract_call", address: "0x0...01",
          abi: "[" + new Array(70000).fill('null').join(',') + "]",
          function: "f", args: [] }];
```

This shape passes `validate_action_kind_allowlisted("contract_call")`
and serde `deny_unknown_fields` (none unknown), and lands in the
journal. The 64 KiB DoS-guard is defeated.

**Fix:** Either add a `deserialize_with` on `ContractCallAction.abi`
that calls `validate_abi_size`, or call
`executor_evm::action::dry_run_abi_encode` (which subsumes
`validate_abi_size`) from `validate_strategy_output` after the
`from_value::<Action>` succeeds:

```rust
// in validate_strategy_output, after the from_value loop:
for (i, action) in actions.iter().enumerate() {
    if let Action::ContractCall(cc) = action {
        executor_evm::action::dry_run_abi_encode(&cc.abi, &cc.function, &cc.args)
            .map_err(|e| format!("action[{i}] (contract_call): {e}"))?;
    }
}
```

Add a regression test that constructs the oversize JSON directly (not
through the builder) and asserts -32018 with the stable detail prefix.

## Warnings

### WR-01: `block_in_place` invoked from inside `spawn_blocking` thread

**Files:**
- `crates/strategy-js/src/sandbox.rs:766-769, 1047-1048, 1154-1156, 1222-1224`

**Issue:** Each EVM host binding wraps the alloy call in
`tokio::task::block_in_place(|| handle.block_on(...))`, but the
`Sandbox::execute` function is itself called from
`tokio::task::spawn_blocking` (`tools.rs:288-304`). `block_in_place`'s
contract requires being on a multi-thread runtime worker thread; from a
blocking-pool thread it is at minimum a misuse of the API and on some
Tokio versions panics with "can call blocking only when running on the
multi-threaded runtime".

The CONTEXT D-04 spec explicitly says:

> "Inside the existing Phase-3 `spawn_blocking` closure, EVM host
>  functions call `tokio::runtime::Handle::current().block_on(...)`."

i.e., `Handle::current().block_on(...)` directly, without
`block_in_place`. The current implementation deviates and adds a
`block_in_place` layer. The fact that the Phase-3 wall-clock test budget
(2s) keeps things short enough that latent panics may not be triggered
in the existing test suite does not make this safe.

**Fix:** Drop the `block_in_place` wrapper:

```rust
let result = match tokio::runtime::Handle::try_current() {
    Ok(handle) => handle.block_on(dispatch),  // direct, per D-04
    Err(_) => { /* unchanged transient runtime fallback */ }
};
```

Verify with a stress test that runs many `ctx.evm.*` calls in a single
strategy.

### WR-02: D-02 isolation comment vs reality — strategy-js transitively pulls alloy

**Files:** `crates/strategy-js/Cargo.toml:18-23`,
`crates/executor-evm/src/lib.rs:46`

**Issue:** D-02 says "strategy-js stays alloy-free". The `cargo tree -p
strategy-js | grep '^alloy'` smoke-test from the focus area would fail
because `strategy-js` depends on `executor-evm` which re-exports
`alloy::providers::DynProvider`. While the strategy-js source code does
not `use alloy::*` directly, the workspace dep graph does include alloy
transitively from strategy-js. The comment in
`crates/strategy-js/Cargo.toml:18-23` ("strategy-js does NOT depend on
alloy directly") is correct but misleading — the intent of D-02 was to
keep alloy out of the strategy-js compile unit, which is also met (the
re-exported types are zero-cost). Mark the test as
"strategy-js source has no `use alloy::`" instead of `cargo tree |
grep`, or accept the transitive re-export as the contract.

**Fix:** Update D-02 wording in CONTEXT (or the related test) to
"strategy-js source does not name alloy directly". The current code is
correct; the documentation is overstating the isolation.

### WR-03: `block_number_resolved` payload field promised by D-13 is silently absent

**Files:** `crates/strategy-js/src/sandbox.rs:801-808, 1086-1092,
1184-1189`

**Issue:** D-13 specifies `payload_json` for ctx.evm.* journal rows
includes `block_number_resolved` ("the integer the provider actually
queried, if available"). The current implementation never populates
this field — the payload only contains `block_tag` (the verbatim agent
input). 04-02-SUMMARY documents this deferral as a NOTE, but there is
no corresponding update to D-13 in CONTEXT, and the payload schema
goldens (if any are wired) implicitly accept the missing field. This is
a documentation/contract drift, not a correctness bug.

**Fix:** Either land the resolved-block-number lookup (one extra
`eth_blockNumber` call when the agent passed `"latest"` / `"pending"`),
or update D-13 to mark the field optional and explicitly drop it from
the v1 payload.

### WR-04: Revert-reason text is agent-controlled and reaches `error.message` unsanitized

**Files:** `crates/executor-evm/src/error.rs:28-32`,
`crates/executor-evm/src/read.rs:200-204`

**Issue:** `EvmError::Revert.reason` is decoded from contract revert
bytes via `try_extract_revert_reason` and embedded directly into the
`Display` impl as `format!("evm revert: {reason}")`. The reason string
is whatever the contract emitted via `revert("...")` — an attacker who
controls the called contract can craft an arbitrary UTF-8 string
(including newlines, ANSI escape sequences, fake error prefixes like
`"evm rpc error: transport"`, or up to several KiB of attacker-chosen
text since there is no length cap on the decoded reason). This text
reaches `error.message` and `data.detail` on the wire.

D-12 explicitly says "decoded reason (if available) appended to stable
`data.detail` prefix" — i.e. this is intentional. The risk is:
1. Log poisoning (newlines in the reason corrupt downstream JSON-line
   log parsers).
2. Confusion with stable taxonomy strings (a malicious contract returns
   `revert("transport")` and the wire shows `"evm revert: transport"` —
   distinguishable from `"evm rpc error: transport"` only by prefix).
3. No upper bound on `reason` length — a contract that reverts with a
   64 KiB string passes through unchanged.

**Fix:**
- Strip control characters (`\n`, `\r`, `\t`, ANSI ESC) from the decoded
  reason before embedding.
- Cap `reason.len()` at e.g. 256 bytes; truncate with an ellipsis marker.
- Document that revert reasons are NOT trusted input.

### WR-05: `validate_address` short-circuit incorrectly accepts mixed-case strings with no alphabetic chars at all

**Files:** `crates/executor-evm/src/action.rs:53-65`

**Issue:** The short-circuit logic:

```rust
let has_alpha = body.chars().any(|c| c.is_ascii_alphabetic());
let all_lower = body.chars().all(|c| !c.is_ascii_alphabetic() || c.is_ascii_lowercase());
let all_upper = body.chars().all(|c| !c.is_ascii_alphabetic() || c.is_ascii_uppercase());
if !has_alpha || all_lower || all_upper {
    return Address::from_str(s).map_err(|e| ...);
}
```

The `!has_alpha` branch accepts an all-digit body unconditionally. For
the canonical zero-address (40 zeros) this is correct. But for an
all-digit-but-wrong-length body, `Address::from_str` rejects — fine.
However, the function does NOT first validate that `body.len() == 40`
or that `body` consists of hex digits before the `parse_checksummed`
call, so a strange input like `"0xZZZZ...ZZZ"` (mixed case Z's) hits
the `!has_alpha` branch only if it has no alphabet (`Z` is alpha, so
this case is fine), but a lowercase `"0xzzzz...zzz"` body would be
hex-valid AND all-lowercase → falls through to `Address::from_str`,
which accepts. That's correct. The issue is only structural — the
function's logic is subtle and not unit-tested for the mixed-non-hex
case.

**Severity:** Minor — `Address::from_str` is the bottom-line authority
and rejects malformed inputs. Documenting the validation invariant or
adding `body.bytes().all(|b| b.is_ascii_hexdigit())` early-returns
would make the logic clearer. Same observation applies to
`address::checksum` (executor-evm/src/address.rs:74-108) which DOES
gate on hex-digit early.

**Fix:** Mirror `address::checksum`'s early-hex-digit check in
`action::validate_address` for clarity and defense-in-depth.

### WR-06: Stale comment in `tools.rs::strategy_run` about provider-build-failure handling

**Files:** `crates/executor-mcp/src/tools.rs:271-282`

**Issue:** The comment says "Provider build failure (e.g. URL parse
error) does NOT fail the whole run — strategies that don't call
ctx.evm.* should still succeed." But by the time `strategy_run` is
invoked, the EvmConfig has already been validated at server boot
(`from_config → evm_config()? → EvmConfig::from_raw`), so URL parse
failure cannot reach this site. The actual lazy `evm_provider().await`
inside the call only fails if reqwest connection-builder fails (which
is essentially impossible for a parsed URL). The comment is misleading
and the `.ok()` swallowing of the result hides any real failure.

**Fix:** Update the comment to reflect that URL/timeout validation
already ran at boot and that `.ok()` here only suppresses
near-impossible reqwest construction errors. Consider propagating the
error instead of `.ok()` so a real failure surfaces as -32017 rather
than being silently swallowed and producing a confusing
"no provider configured" message later.

### WR-07: `qjs_value_to_json` allows BigInt at the args walker for ctx.evm.readContract while D-03 forbids it

**Files:** `crates/strategy-js/src/sandbox.rs:725-738, 1845-1908`
(qjs_value_to_json)

**Issue:** When extracting `args` for `ctx.evm.readContract`, the code
calls `qjs_value_to_json(&args_value)` which has a `Type::BigInt =>
Err("BigInt is not supported ...")` branch. Good — BigInt args are
rejected. However, the rejection happens AFTER the JS BigInt has
crossed the host boundary, and the resulting error message is
`"args: BigInt is not supported in strategy returns (Pitfall 8)"` —
which speaks of "returns" even though this is an INPUT path. The error
is also generic ("not supported") rather than the D-03 stable-string
contract ("amount must be a decimal string, got BigInt — use
ctx.units.parseUnits(...)").

The action-builders (`require_string_field`) DO emit the D-03 stable
message. The readContract path does not.

**Fix:** Pre-walk the args array, detect BigInt at args[i] level, and
emit the D-03 stable rejection message for parity with the builders.

### WR-08: Action builders accept addresses that fail strict EIP-55 only on `address` arg of `contractCall`, but `dyn_abi.rs::js_value_to_dyn_sol` accepts ANY case for ABI args of type `address`

**Files:** `crates/executor-evm/src/dyn_abi.rs:46-55`,
`crates/executor-evm/src/action.rs:49-65`

**Issue:** `validate_address` (action.rs) is strict EIP-55 lenient
(accepts lowercase / uppercase / strict checksum; rejects
mixed-case-bad). But the ABI-arg path (`js_value_to_dyn_sol`,
`DynSolType::Address`) calls `Address::from_str` directly without
checksum strictness — accepting ANY case combination including
mixed-case-bad-checksum.

So:

```js
// REJECTED (D-09 strict):
ctx.actions.contractCall({ address: "0xAbCdEf...bad", abi:..., function:..., args:[] })

// ACCEPTED (lenient — the args[] path):
ctx.actions.contractCall({ address: "0x0000...01", abi:..., function:"f",
    args: ["0xAbCdEf...bad"] })  // ABI arg of type address: case ignored
```

Comment at dyn_abi.rs:50 acknowledges this:
> "Accept lowercase / uppercase / EIP-55 — the action validator at
>  the MCP boundary enforces checksum strictness for D-09."

But the MCP-boundary action validator only checks the top-level
`address` field of each action variant — it does NOT walk into ABI
args. So address-typed ABI args sail through without EIP-55 check,
even though they appear in the same outgoing `Action` payload.

**Severity:** Medium. This is a consistency gap, not a security flaw —
ABI args are encoded into calldata at Phase 5 normalization and an
all-lowercase or mixed-bad-checksum address still resolves to the same
bytes. The risk is user typo: a strategy author who flips one case bit
in an ABI-arg address gets no warning, but they would get one for the
top-level address. Plan-level decision; either:

**Fix:** Either route the ABI-arg path through `validate_address`
(strict-lenient) and reject mixed-bad, OR document that ABI-arg
addresses are NOT EIP-55-checked and recommend
`ctx.address.checksum(...)` in strategy code.

## Info

### IN-01: `unused` import / dead-code style — module re-exports

**Files:** `crates/executor-evm/src/lib.rs:27-46`

The `pub use` block re-exports `dry_run_abi_encode`,
`validate_abi_size`, etc. at crate root. With BR-02 fixed (calling
`dry_run_abi_encode` from validate_strategy_output), this surface is
needed; without it, `dry_run_abi_encode` and `validate_abi_size` are
re-exported but only consumed inside `executor-evm` itself. Worth
auditing post-BR-02-fix.

### IN-02: ZERO_ADDRESS reassignable in JS — already noted as NOTE-1 closure

**Files:** `crates/strategy-js/src/sandbox.rs:466-468`

`ctx.address.zeroAddress` is installed via `addr_obj.set("zeroAddress",
...)` with the default property descriptor (writable, configurable).
Strategies can reassign it. NOTE-1 closure in 04-04-SUMMARY documents
this as accepted. For defense-in-depth, consider `Object.freeze(addr_obj)`
after installing all properties — costs nothing, hardens contract.

### IN-03: `EvmError::Encode` category constants are `&'static str` — good — but `Decode { category: "rethrown" }` placeholder if BR-01 fix is taken

**Files:** `crates/executor-evm/src/error.rs:21-24`

If BR-01 is fixed by re-classifying JS exception messages back into
EvmError variants, the Decode/Encode `category: &'static str` field
makes round-tripping awkward (the JS message only carries the category
string, not its `'static` lifetime). A pragmatic mitigation is a
`category: "rethrown_from_js"` sentinel — works, but loses the original
category. Alternatively, change `EvmError::Decode/Encode` to
`category: Cow<'static, str>` so a re-thrown variant can carry the
runtime string. Defer until BR-01 is being fixed.

---

_Reviewed: 2026-04-27_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
