# Protocol consistency status

Updated: 2026-09-10T02:03:56.369919+00:00

## Live sync verification

Updated: 2026-09-10T02:03:56.369919+00:00

- Local neo-rs tip (approx): `1528989`
- Note: PASS: 230 goldens <= tip 1528989 match committed C# checkpoints (sparse L2 evidence; not contiguous L3)

Auto-updated by `scripts/monitor-golden-compare-while-sync.py`.

## Claim language (current)

**Do not claim tip-level / 100% MainNet protocol equivalence.**

What is true today:

| Layer | Status |
|-------|--------|
| L0/L1 vectors & block-byte compliance tests | Present in repo / CI |
| L2 C# golden state-root checkpoints | Committed: `tests/fixtures/protocol_consistency/csharp_stateroot_goldens.jsonl` (230 heights) |
| L2 online golden drift check | `python scripts/verify-protocol-consistency.py verify-goldens-online` |
| L2 sparse local vs goldens while syncing | In progress — all goldens ≤ current tip matched |
| L3 contiguous Rust↔C# `[0, H]` | Sync running; contiguous fail-closed compare not yet claimed |
| L4 continuous tip validation | Tooling exists; needs tip catch-up |

## Known open divergences (from in-tree reproducers)

These heights previously produced Rust roots ≠ C# when replayed against a local full-state DB. They are **blockers** for any claim past the first failing height. Fix in ascending height order:

| Height | Notes (from `mainnet_block_*_repro.rs`) |
|--------|------------------------------------------|
| 980196 | **Likely fixed (I3-01, 2026-09-07):** multi pending `onNEP17Payment` chained `calling_context` broke `CalledByEntry` after NEO.transfer + GAS mint. Unit regression: `pending_native_callbacks_share_native_invoke_calling_context` (**PASS**). **Still needs** full-state DB replay / live root at tip ≥ 980196 to close the claim. |
| 1465790 | Documented Rust≠C# root in repro header |
| Others | Additional reproducers under `neo-core/tests/mainnet_block_*_repro.rs` — treat any failing assert as a gate |

Until each is fixed **and** re-verified with fail-closed compare, the claimable contiguous range cannot safely include that height.

## How to raise the claimable `H`

1. Keep syncing neo-rs MainNet with StateService full state (running under WSL).
2. Sparse check while syncing:

```bash
python scripts/verify-protocol-consistency.py compare-goldens \
  --local http://127.0.0.1:10332
```

3. Fail-closed contiguous compare:

```bash
python scripts/verify-protocol-consistency.py compare-rpc \
  --local http://127.0.0.1:10332 \
  --reference http://seed1.neo.org:10332 \
  --start 0 --end <local_height>
```

4. On first mismatch: stop; open/fix reproducer; re-sync from last good height; repeat.
5. Attach the JSON report under `docs/protocol-consistency/reports/` and update this file with the new `[0, H]`.

## Authoritative policy

See `docs/PROTOCOL_CONSISTENCY.md`.
