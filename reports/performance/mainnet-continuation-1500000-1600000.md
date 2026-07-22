# MainNet Continuation 1,500,000-1,600,000

The durable full-state node continued with the validated `16,384` coordinated
change budget through height `1,600,000` on 2026-07-16.

- Ledger and StateService heights: `1,600,000`.
- MPT failures: `0`.
- Local root: `0xe8e59ae6c7b10318c66d75d781188b7661ad17071448a71bcf0cb566bc6702a3`.
- `seed1.neo.org:10332 getstateroot(1600000)` returned the same root.
- Overall rate: `183.23` blocks/s.
- Transaction-bearing rate: `712.35` blocks/s (`100,197` transactions).
- Finalization: `485.13 s` of `546.18 s` importer driver time.
- Final 10,000-block commit sample: seven MDBX transactions and `60.59 s`.
- Empty blocks alone: `43,569.18` blocks/s.

The release probe reopened the database after shutdown and read the matching
height/root. Native VM execution consumed `52.70 s` total (`~526 us` per
transaction); durable MDBX/state publication remained the dominant cost. This
is correctness-positive continuation evidence, not full MainNet-tip or
1,500-block/s speed proof.

Artifacts:

- `reports/performance/mainnet-profile-1500000-1600000-node.log`
- `reports/performance/mainnet-profile-1500000-1600000-time.txt`
- `data/neo-v3101-staged-replay/runs/isolated-920097`
