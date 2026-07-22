# MainNet Continuation 1,400,000-1,500,000

The durable full-state node continued with the measured `16,384` coordinated
change budget through height `1,500,000` on 2026-07-16.

- Ledger and StateService heights: `1,500,000`.
- MPT failures: `0`.
- Local root: `0x058f29d359ec64eb394d061f44c9ba3e4991b41795ce745aaa3d8d55c9ff26af`.
- `seed1.neo.org:10332 getstateroot(1500000)` returned the same root.
- Overall rate: `250.81` blocks/s.
- Transaction-bearing rate: `910.61` blocks/s (`57,494` transactions).
- Finalization: `357.32 s` of `400.17 s` importer driver time.
- Final 10,000-block commit sample: seven MDBX transactions and `38.93 s`.
- Empty blocks alone: `43,180.87` blocks/s.

The release probe reopened the database after shutdown and read the matching
height/root. Native VM execution consumed `31.43 s` total (`~547 us` per
transaction), while durable finalization remained the dominant cost. This is
correctness-positive continuation evidence, not a full MainNet-tip or
1,500-block/s speed proof.

Artifacts:

- `reports/performance/mainnet-profile-1400000-1500000-node.log`
- `reports/performance/mainnet-profile-1400000-1500000-time.txt`
- `data/neo-v3101-staged-replay/runs/isolated-920097`
