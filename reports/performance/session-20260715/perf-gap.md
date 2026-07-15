# Performance gap to sustained 1500–2000 blocks/s (production claim)

## What already hits the band
- Workload: MainNet archive import, `full_state=false`, `coordinated=false`, tmpfs, verify=false.
- Dense 290k–300k multi-run control mean ≈ **1,618 blocks/s** (3-run interleaved A/B control).
- Overall 100k–300k ≈ **11.5–11.8k blocks/s**.
- Empty-block path ≈ **50k blocks/s**.

## Production-default path gap
- Coordinated full-state durable high-height (e.g. h811–821) remains ~**50–60 blocks/s** in prior retained evidence; finalization/store commit dominates.
- Dense full-state stateful window previously ~**569 blocks/s** with MPT apply ~17s / 10k blocks.

## Named next hotspots
1. **Coordinated deferred import batch**: one atomic Ledger+StateService commit for large projected-change batches → split at adaptive work budget without weakening durability.
2. **Finalization backing I/O**: hundreds of thousands of unique misses per dense window; process-local bloom rejected earlier (startup cost); need batch-locality / deferred transform design.
3. **Transaction `load_execute`**: secondary after StateService on dense windows.

## Evidence location
- Scratch: `/tmp/grok-goal-2b0db41c13bc/implementer/baseline`, `perf-ab/parallel-batch`, `perf-ab/negative-cache`
- In-repo: `reports/performance/session-20260715/SUMMARY.md`
