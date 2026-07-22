# MainNet Continuation 1,710,500-1,720,000

Captured 2026-07-16 with durable MDBX and
`NEO_MDBX_WRITE_INTENT_READ_THREADS=16`. The node reached the target through
the fixed headers-first sync path and passed the post-run reopen/reference
probes with zero MPT failures.

- Ledger and StateService heights: `1,720,000`.
- Local/reference root: `0xbce11adba3a44c3b1d4646407d9e4361f35ddc85fcea2e4936ccfd898481edb6`.
- `seed1.neo.org:10332 getstateroot(1720000)` matched exactly.
- Advanced blocks: `9,500`.
- End-to-end elapsed time: `91.066 s` (`104.32 blocks/s`).
- RSS peak: approximately `22.5 GB`.

The write-intent reader pool improves the sparse lookup phase on this warm
window, but the result remains far below the requested 1,500-2,000 blocks/s.
Durable MPT/MDBX page writes and commit fences remain the dominant hotspot.

Artifacts:

- `reports/performance/mainnet-profile-1710500-1720000-writeintent16-node.log`
- `reports/performance/mainnet-continuation-1710500-1720000-writeintent16.json`
- `data/neo-v3101-staged-replay/runs/isolated-920097`
