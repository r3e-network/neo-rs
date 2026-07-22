# Full-State Finalization A/B

Captured 2026-07-16 from the retained height-1,000 MainNet checkpoint. The
control uses the default C#-compatible eager full-state policy. The candidate
sets `[state_service].defer_full_state_finalization = true`, which defers node
materialization inside ordered full-state batches while keeping reference
resolution eager.

## Correctness

- Replay range: heights 1,001 through 30,000 (29,000 blocks, 166 transactions).
- Checkpoint root at height 1,000: `0x60d823c9201730107590ad98684f0369c65b6f0a050a924244f078f0f345e27e`.
- Control and candidate Ledger/StateService heights: `30,000`.
- Control and candidate final root: `0x61062a078463b88fa18eac86722dd8d3faf50a6ec2ee8e5e70c06c16546a9347`.
- Both roots matched `seed1.neo.org:10332 getstateroot(30000)`.
- Reopened MDBX probes at heights 1,000, 5,000, 10,000, 20,000, and 30,000
  returned the same roots and storage values for both variants.
- The complete height-30,000 storage dump contained 221 entries in each
  variant and had normalized SHA-256
  `abede1f4d6c296661a54b40ff3d88880fd97df0cd4c813e84817d7d5ade140de`.
- A direct `neo-db-probe --mpt-dump-root 30000 --dump-limit 1000` comparison
  found 221 non-truncated leaves in both snapshots. Canonicalizing the full
  `mpt_root_storage` JSON (excluding the database path) produced the identical
  SHA-256 `d0a383c313005636f2284ae266f477f1e91a0292faa9af3b3343301a3adc2ed2`.
- The exact deferred replay log, release node, and release database probe were
  hashed after the comparison:

  | Artifact | SHA-256 |
  |---|---|
  | `fullstate-exact-h1k-30k.log` | `5ff49b0236954e13174f73151a5fcce2fbcc5979ced8b60bdba3f3d8c1a17184` |
  | `target/release/neo-node` | `3dd01bf225987d7c466f3f7d85ed023ad5f8b8ff79f7a9a98c6107a2a8e84143` |
  | `target/release/neo-db-probe` | `12465b86696c404c6a9a88c368b0fddf7ad4626e0346f9f537b319ead16ebe96` |
- Focused tests also compare every reachable node byte and reference count
  across eager and deferred histories, including retry-after-read-failure.

The candidate preserves the complete eager raw namespace by recording every
serialized mutation and resolving references in one ordered batch. The eager
policy remains the production default until this mode has wider replay and
durable-disk evidence.

## Performance

| Variant | Wall time | Finalization | Final 9,000-block window | Final-window MPT apply | Final-window trie commit | Repeated ancestors |
|---|---:|---:|---:|---:|---:|---:|
| Eager control | 6.10 s | 5.313 s | 3,183 blocks/s | 2.452 s | 1.318 s | 111,495 |
| Deferred opt-in | 4.07 s | 3.086 s | 5,094 blocks/s | 1.386 s | 0.820 s | 0 |

The candidate's full-run transaction-bearing rate was 7,012 blocks/s. These
are isolated durable MDBX checkpoint runs, not sustained MainNet throughput or
proof that the default configuration reaches the 1,500-2,000 blocks/s target.

Artifacts:

- Control log: `fullstate-eager-h1k-30k.log`, SHA-256
  `358f1d821be23927c40b7929b7726a6be7facf39fe67c68d614e2b877a8f77da`.
- Opt-in log: `fullstate-optin-h1k-30k.log`, SHA-256
  `882981a286318479862a6f6e4a2f7df818179bc51fadd738c03f8fdd1d3aad20`.
- Release node binary used by the opt-in run, SHA-256
  `8716ccbe3a5d706e6102c10bc7233dcd1f15533a7854cd44aedd15fe6f1ca091`.

This A/B is optimization evidence only. OpenSpec replay tasks 4.4 and 4.5
remain open until the staged hardfork checkpoints and full MainNet tip replay
are completed with the default-compatible storage policy.
