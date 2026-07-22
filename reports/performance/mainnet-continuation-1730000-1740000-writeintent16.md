# MainNet Continuation 1,730,000-1,740,000

Captured 2026-07-16 with durable MDBX and
`NEO_MDBX_WRITE_INTENT_READ_THREADS=16`.

- Ledger and StateService heights: `1,740,000`.
- Local/reference root: `0xdb77ab8f1c9138f5ef369aea2f9b834ebf1ffc110b4d69d2d07d1f7597bb631d`.
- `seed1.neo.org:10332 getstateroot(1740000)` matched exactly.
- Advanced blocks: `10,000`.
- End-to-end elapsed time: `115.364 s` (`86.68 blocks/s`).
- MPT failures: `0`.
- RSS peak: approximately `35.7 GB`.

The write-intent pool preserved correctness and reduced sparse lookup pressure,
but durable MPT/MDBX page writes and commit fences still dominate the window.

Artifacts:

- `reports/performance/mainnet-profile-1730000-1740000-writeintent16-node.log`
- `reports/performance/mainnet-continuation-1730000-1740000-writeintent16.json`
- `data/neo-v3101-staged-replay/runs/isolated-920097`
