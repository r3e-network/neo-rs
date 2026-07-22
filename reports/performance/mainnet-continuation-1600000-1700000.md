# MainNet Continuation 1,600,000-1,700,000

The durable full-state node continued with the validated `16,384` coordinated
change budget through height `1,700,000` on 2026-07-16.

- Ledger and StateService heights: `1,700,000`.
- MPT failures: `0`.
- Local root: `0xbe328747a11cf1b004f6e75524160bc8dbfddaca666f17cef25bc6fe50bed95e`.
- `seed1.neo.org:10332 getstateroot(1700000)` returned the same root.
- Overall rate: `152.26` blocks/s.
- Transaction-bearing rate: `436.93` blocks/s (`132,769` transactions).
- Finalization: `558.25 s` of `657.37 s` importer driver time.
- Final 10,000-block commit sample: 26 MDBX transactions, `152.97 s`
  total backing commit time, and 3,221,438 entries.
- Empty blocks alone: `43,383.86` blocks/s.

This transaction-dense window is a durable MDBX stress baseline. Native VM
execution consumed `87.77 s` total (`~661 us` per transaction), while backing
commit/page-write work consumed the majority of wall time. RSS peaked at about
`44.9 GB`; disk remained healthy with roughly `983 GB` free. The release probe
reopened the database and read the exact reference root.

Artifacts:

- `reports/performance/mainnet-profile-1600000-1700000-node.log`
- `reports/performance/mainnet-profile-1600000-1700000-time.txt`
- `data/neo-v3101-staged-replay/runs/isolated-920097`
