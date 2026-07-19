# Authoritative Pack Compaction Workspace Guard

Date: 2026-07-19

## Outcome

The current v3 index compactor materializes and globally sorts every input
record. A real 225M-entry authoritative pack drove the old path to about 35 GiB
RSS and blocked catch-up behind a single maintenance worker. The new guard
computes a conservative workspace estimate from immutable run metadata before
reading record mmaps, allocating the materialization vectors, or creating an
output file.

For the selected 16-run MainNet merge, the updated build estimated
44,106,026,656 bytes against the configured 1,073,741,824-byte bound and
deferred the plan. This is a fail-closed resource guard, not completed
compaction: the debt remains and a bounded streaming merge is still required.

## Corpus And State

- MDBX: `runs/ab-search-h3377022`
- Authoritative pack: `authoritative-packs-3287022`
- Canonical height: 3,382,022
- Authority epoch: 263
- Frames: 264
- Live runs: 24
- Live index entries: 225,048,697
- State root: `0x39f94e5d8ac04a687861ea8d4838141092d54e3f789d21a4d5aece874db1f0d5`
- Both `seed1.neo.org` and `seed2.neo.org` returned the same root.

The committed-prefix scrub completed before this guard run with 225,073,562
rows, no tombstones, and a reachable marker-bound root.

## Direct Maintenance Probe

Command:

```bash
/usr/bin/time -v env \
  NPV_MDBX=data/neo-v3101-staged-replay/runs/ab-search-h3377022 \
  NPV_PACK=data/neo-v3101-staged-replay/authoritative-packs-3287022 \
  target/release/neo-pack-verify \
  --mode authority --network-magic 0x334F454E \
  --max-index-memory-mb 1024 --samples 1 --walk-cap 1 --maintain
```

Observed:

- Wall time: 0.93 s
- Maximum RSS: 644,224 KiB
- Exit: 1 with typed `CompactionWorkspaceExceeded`
- No `.tmp` or `.pending` output
- No pack, manifest, or tail-run mutation

The nonzero exit is intentional for the explicit maintenance command: it did
not claim to compact work that exceeded its hard budget.

## Node Worker Probe

The real node opened the same marker-bound store and requested background
maintenance while performing a no-op import to the existing height:

```bash
/usr/bin/time -v target/release/neo-node \
  --config data/neo-v3101-staged-replay/neo_mainnet_validate_authoritative_packs.toml \
  --storage-path data/neo-v3101-staged-replay/runs/ab-search-h3377022 \
  --import-chain data/neo-v3101-staged-replay/fast-sync-cache/chain.0.acc/chain.0.acc \
  --stop-at-height 3382022
```

Observed:

- Worker warning included estimate 44,106,026,656 and bound 1,073,741,824.
- Wall time: 0.68 s
- Maximum RSS: 716,740 KiB
- Imported blocks: 0
- Exit: 0
- Writer remained usable; maintenance was deferred rather than poisoned or
  retried in a CPU loop.

## Immutable Evidence

- `frames.pack` SHA-256:
  `9a8ff7b0ef9aab84b784ae986d47caf68dd3f6b7c9eca985ea1feece3a0620ed`
- `manifest-00000000000000000019.man` SHA-256:
  `dcaa3164a6d7b0543146ba6ac29c92f210891edc735545bc1cca30ceec25dd0f`
- `run-00000000000000000263.idx` SHA-256:
  `5e31384c5cc6b9b8fe999c9e3381445d4cf7cd3c9c8a1d6cd58d89118b0b6420`
- `neo-node` SHA-256:
  `6b1c80358e092d06c16bede478b61ca85ea01a1b2dfb27879ecc7addbf484499`
- `neo-pack-verify` SHA-256:
  `59616e033d4ea7bcfcad8adc75d91c80e7881000cff711c1d7608b650a5c9180`

## Host

- CPU: 8 virtual CPUs, Intel Core Ultra 9 285K host model
- Memory: 62 GiB
- Storage: 2 TiB VMware virtual rotational device, ext4
- Kernel: Linux 6.17.0-35-generic x86_64
- Durability: ordinary `sync_data` and directory-sync paths remain enabled

## Remaining Work

This guard removes the OOM and unbounded-retry failure mode but cannot reduce
compaction debt. Task 2.7 remains open. The production implementation still
needs a bounded k-way merge over already sorted inputs, streaming record output,
a bounded external-memory xor-filter build (or a versioned sharded filter),
crash injection, reopen/scrub parity, and a sustained MainNet RSS gate.

No 1,500-2,000 blocks/s claim follows from this result.
