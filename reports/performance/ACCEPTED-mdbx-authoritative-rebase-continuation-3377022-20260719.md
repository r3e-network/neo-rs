# Accepted: compact MDBX rebase and continuation replay

Date: 2026-07-19 (Asia/Shanghai)

This report records the first continuation replay after the offline logical
rebase of the authoritative MainNet StateService database. It is a
correctness and storage-footprint result, not a claim that the node has
reached the requested 1,500-2,000 blocks/s target.

## Inputs

- Network/protocol: Neo N3 MainNet, v3.10.1 configuration
- Source MDBX: `data/neo-v3101-staged-replay/runs/authority-ab-h3287022`
- Rebased MDBX: `data/neo-v3101-staged-replay/runs/authority-ab-h3287022-rebased-20260719`
- Authoritative packs: `data/neo-v3101-staged-replay/authoritative-packs-3287022`
- Chain archive: `data/neo-v3101-staged-replay/fast-sync-cache/chain.0.acc/chain.0.acc`
- Replay: blocks `3,372,023..3,377,022` (5,000 blocks)
- Durability: normal MDBX durable commits; authoritative pack mode with random
  point mmap enabled

The rebase copied Main and `neo_node_metadata` exactly and retained only the
non-node StateService metadata. It excluded exactly 33-byte `0xf0 || hash`
keys from `neo_state_service`; pack bytes remain the authority for that
namespace.

## Storage result

| Measurement | Source | Rebased |
|---|---:|---:|
| MDBX file | 124,554,051,584 bytes | 4,294,967,296 bytes |
| Logical reduction | | 96.55% |
| Source transaction | 348,474 | |
| Excluded node rows | 219,011,833 | |
| Excluded node key bytes | 7,227,390,489 | |
| Excluded node value bytes | 45,438,335,269 | |

The rebase report is
`mdbx-rebase-authority-ab-h3287022-20260719.json`. Its ordered SHA-256
digests cover every retained row in Main, `neo_node_metadata`, and the
retained StateService metadata. The source environment was left untouched
until continuation and scrub verification completed.

## Continuation replay

- Imported: 5,000 blocks
- Transaction blocks: 1,163
- Transactions: 1,703
- Empty blocks: 3,837
- Wall-clock import: 9.152261401 s
- Driver wall-clock: 9.178142348 s
- Throughput: **546.313 blocks/s**
- Transaction-bearing throughput: 340.434 blocks/s
- Empty-block path: 37,628 blocks/s
- Finalization: 5.617230948 s
- Finalization store commit: 5.617229871 s
- Native VM transaction load/execute: 2.613058 s total
- MDBX commit-window total: 2.311350 s
- MDBX cursor visit/write: 2.105392/2.101223 s
- MDBX committed transactions: 2
- MDBX entries: 20,347 (20,058 puts, 289 deletes)
- StateService node puts: 273,212
- StateService finalization major faults: 5,521
- StateService finalization read bytes: 22,614,016

The machine-readable profile is
`mainnet-authoritative-rebased-mdbx-3372022-3377022-20260719-profile.json`,
with `/usr/bin/time -v` evidence in
`mainnet-authoritative-rebased-mdbx-3372022-3377022-20260719-time.txt`.

The adjacent accepted 5,000-block baseline measured 510.731 blocks/s, but
the transaction mix is not identical. Therefore this result is reported as a
continuation/reopen gate, not as a causal A/B speedup claim. The dominant
remaining hotspot is still MDBX cursor visitation/write and finalization
queue wait; the compact database alone does not remove that path.

## Correctness gates

- Rebased tip before replay: height `3,372,022`, root
  `0x86a5c2b884b7f04bfbb87e9d52fad86f0af2c7e75704853dcb090ca3931549b1`.
- Continued tip: height `3,377,022`, root
  `0xd2340aa41c03ad25f4269cccac5dffa5a5c3e51b389edb6ee5ae3347e5879a57`.
- `seed1.neo.org` and `seed2.neo.org` independently returned the exact
  continued height and root.
- `neo-node --check-all` reopened the compact MDBX and authoritative pack.
- Full authority verification and scrub passed with the node's configured
  1,024 MiB index-memory bound:
  - 262 frames, 6 index runs, 224,768,425 index entries
  - 224,800,876 rows/puts, zero tombstones
  - payload bytes `55,318,836,580`
  - value bytes `46,776,403,292`
  - reachable root node at `0xf0 || 579a87e5...0a34d2`
- Scrub wall time: 34.75 s; exit status 0.

The verifier's default 256 MiB bound failed closed before reading the pack;
rerunning with the configured 1,024 MiB bound passed. This is an operator
configuration requirement, not a data failure.

## Reproducibility hashes

- `neo-node`: `f0b1f4f7b20dfc34b196e0a7fe7e1dc9bc1e9aec59efb11c8ce4df7747f8d367`
- `neo-pack-verify`: `cb50861ea025424caa128afbf36c3ae47a264c6ea51921d843d34b30c3f3280d`
- `neo-db-probe`: `c058e21e7a4077ce5c9abc5b6282c4e6f7bddcc3666b3f750a0b450638e325f4`
- Rebase report: `1cdae18b11d91c1e04c7c38793210d609e418abbdb63d28e2d5ddd5dd5374fdb`
- Continuation profile: `2bf104fa895e3dc1410010d0cff60b55988aeaed7d0123c0c49f57b00398714e`

The requested 1,500-2,000 blocks/s production gate remains open. The next
optimization should target the measured finalization/MDBX write path and be
accepted only after another exact-root continuation and scrub cycle.
