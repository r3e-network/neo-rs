# MainNet Continuation 1,770,000-1,780,000

Captured 2026-07-16 with durable MDBX, deferred full-state finalization, and
`NEO_MDBX_WRITE_INTENT_READ_THREADS=16`.

- Ledger and StateService heights: `1,780,000`.
- Local/reference root: `0x0dafbca58a202f3549fec514f441445c70520153b3acbee3ad5e20d803fc0f27`.
- `seed1.neo.org:10332 getstateroot(1780000)` matched exactly after reopen.
- Advanced blocks: `10,000`.
- End-to-end elapsed time: `80.221 s` (`124.65 blocks/s`).
- Height-poll average: `166.26 blocks/s` (polling is not the end-to-end proof).
- MPT failures: `0`.
- Prometheus samples: unavailable for this run because telemetry metrics were disabled.

This durable window is correctness-positive but remains far below the required
1,500-2,000 blocks/s production band. The run is retained as a profiling
baseline; it does not promote any unsafe sync or write mode.

Artifacts:

- `reports/performance/mainnet-profile-1770000-1780000-writeintent16-node.log`
- `reports/performance/mainnet-continuation-1770000-1780000-writeintent16.json`
- `data/neo-v3101-staged-replay/runs/isolated-920097`
