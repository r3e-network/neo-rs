# MainNet Continuation 1,700,000-1,710,500

Captured 2026-07-16 with the durable MDBX default and the full-state deferred
finalizer. The bounded network probe was stopped after crossing height
1,710,000 so the retained database stayed at a clean checkpoint for the next
continuation.

- Ledger and StateService heights: `1,710,500`.
- MPT failures: `0` in the node log and post-run probes.
- Local root: `0x371f93969463f28e89ea9d016d537410efcaadbd40e2549706303bcfe3737e8f`.
- `seed1.neo.org:10332 getstateroot(1710500)` matched exactly.
- Advanced blocks: `10,500`.
- Driver elapsed time: `320.841 s` (`32.73` end-to-end blocks/s).
- RPC 500-block samples: approximately `99.6-99.8 blocks/s`; these exclude
  the initial durable startup and commit fences.
- RSS peak: approximately `22.6 GB`.

This confirms the previous finding: durable MDBX/MPT page writes and commit
fences dominate the transaction-bearing path. The merge-cursor experiment was
kept opt-in because adaptive sparse fallback did not improve this workload.

Artifacts:

- `reports/performance/mainnet-profile-1700000-1800000-node.log`
- `reports/performance/mainnet-continuation-1700000-1800000.json`
- `data/neo-v3101-staged-replay/runs/isolated-920097`
