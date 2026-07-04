# M2: GasToken.on_persist Network-Fee Mint Guard

**Severity:** MEDIUM (non-consensus, cosmetic)
**Status:** NO FIX NEEDED — Rust code already matches C# behavior

## What the spec-v3100-parity-findings claim

From `claudedocs/spec-v3100-parity-findings.md:122`:

> GAS OnPersist guards network-fee mint with `> 0` instead of always minting;
> negative totalNetworkFee halts instead of faulting
>
> spec: `src/neo/native/gas_token.py:71-77`
> csharp: `neo_csharp/src/Neo/SmartContract/Native/GasToken.cs:55-57`

The finding refers to the **XSPEC Python spec** (`gas_token.py`), which had a guard:

```python
if total_network_fee > 0:
    self.mint(engine, primary, total_network_fee, False)
```

In the Python spec, when `total_network_fee <= 0`:
- Zero: skip mint → no-op (same end result as C#)
- Negative: skip mint → silent no-op (divergence from C#, which faults)

## What C# does (reference)

C# `GasToken.OnPersistAsync` (GasToken.cs:55-57) **always** calls:

```csharp
Mint(engine, account, totalNetworkFee, false);
```

No guard. `FungibleToken.Mint` handles all three cases internally:
- `amount.Sign < 0` → `ArgumentOutOfRangeException` (fault)
- `amount == 0` → return (no-op)
- `amount > 0` → credit balance, raise supply, emit Transfer

## What the Rust code does

`neo-native-contracts/src/gas_token/mod.rs:542-553` — the `on_persist` hook:

```rust
let primary = crate::NeoToken::new().next_block_validator_account(
    &snapshot,
    validators_count,
    primary_index,
)?;
self.gas_mint(engine, &primary, &BigInt::from(total_network_fee), false)
```

**No `> 0` guard.** Always calls `gas_mint`, just like C# always calls `Mint`.

`gas_mint` (lines 221-249) handles all paths correctly:
- `amount < &BigInt::zero()` → returns `Err(CoreError::invalid_operation(...))` — fault, matching C# exception
- `amount.is_zero()` → returns `Ok(())` — no-op, matching C# return
- `amount > 0` → mints GAS, emits Transfer, optional onNEP17Payment — matching C#

## Path-by-path comparison

| `total_network_fee` | C# `OnPersistAsync` | Rust `on_persist` | Match? |
|---|---|---|---|
| `> 0` | Calls `Mint(amount)` → mints GAS | Calls `gas_mint(amount)` → mints GAS | YES |
| `== 0` | Calls `Mint(0)` → returns (no-op) | Calls `gas_mint(0)` → returns Ok (no-op) | YES |
| `< 0` | Calls `Mint(negative)` → throws fault | Calls `gas_mint(negative)` → returns Err (fault) | YES |

## Verdict

**No fix is needed.** The Rust port correctly followed the C# behavior — always calling
the mint function unconditionally — rather than the XSPEC Python spec's guarded
approach. The XSPEC divergence (the `if total_network_fee > 0:` guard that silently
swallowed negative values) was corrected during the Rust implementation.

The "cosmetic" label in the task title is accurate: the divergence existed only in
the intermediate Python spec, not in the Rust code. All three sign paths produce
byte-for-byte compatible results between Rust and C#.

### Practical note

Negative `total_network_fee` is unreachable in valid blocks — it can only occur
through an integer overflow in the NotaryAssisted fee deduction, which would
require a block with implausibly high notary attribute counts. Both C# and Rust
fault on this degenerate input, preserving deterministic rejection.
