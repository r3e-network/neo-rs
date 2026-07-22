# MainNet Continuation 1,720,000-1,730,000

Captured 2026-07-16 with durable MDBX and
`NEO_MDBX_WRITE_INTENT_READ_THREADS=16`.

- Ledger and StateService heights: `1,730,000`.
- Local/reference root: `0xf7ab01a8fb8426cb191b92f02eed18b74cb05290b2e0ac626e489d00e0c3fd66`.
- `seed1.neo.org:10332 getstateroot(1730000)` matched exactly.
- Advanced blocks: `10,000`.
- End-to-end elapsed time: `137.696 s` (`72.62 blocks/s`).
- MPT failures: `0`.
- RSS peak: approximately `32.2 GB`.

The lower rate than the preceding 1,710,500-1,720,000 window reflects denser
transaction/MPT work, not a correctness issue. The write-intent reader pool
remains enabled for the next bounded continuation; durable MDBX commit/page
writes remain the limiting stage.

Artifacts:

- `reports/performance/mainnet-profile-1720000-1730000-writeintent16-node.log`
- `reports/performance/mainnet-continuation-1720000-1730000-writeintent16.json`
- `data/neo-v3101-staged-replay/runs/isolated-920097`
