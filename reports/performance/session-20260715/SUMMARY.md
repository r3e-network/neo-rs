# Session performance & correctness — 2026-07-15

## Baseline (pruning, uncoordinated, tmpfs)

| Metric | Value |
|--------|------:|
| Window | MainNet 100,001–300,000 |
| Overall blocks/s | 11,523.73 |
| Dense 290–300k blocks/s | 1,599.79 |
| Transaction-bearing blocks/s | 1,835.91 |
| Empty blocks/s | 49,793.83 |
| Official root @300k | `0xf0e121ac28b2de37e7e0321b0e0ef926f8b1ff9e85ee6f03c0c6c242a5f31088` |
| Root match | YES |
| MPT failures | 0 |

Dense window already sits in the **1,500–2,000 blocks/s** band for this
workload class (pruning, uncoordinated, tmpfs archive import).

## Retained changes

### 1. Fail-closed typed point reads (`neo-storage`)
- Added `ReadOnlyStoreGeneric::try_get_result`.
- MDBX store/snapshot and `RuntimeStore` override it to surface backend failures.
- Legacy `try_get` remains soft-fail for compatibility.
- Tests: `fallible_typed_point_reads_reject_an_invalid_snapshot_without_changing_legacy_reads`.

### 2. MPT negative cache for proven-absent node keys (`neo-state-service`)
- `MptWriteBatch` consults `absent_from_base` on point and batch lookups.
- Avoids redundant durable re-reads of keys already proven missing in the frozen base.
- Tests: `write_batch_negative_cache_skips_repeated_durable_misses`.
- A/B on dense window: **neutral** (~−1.7% vs single-run control, within noise);
  roots matched every run. Retained as correctness / IO hygiene.

### 3. Fast-sync optional SHA-256 authenticity (`neo-node`)
- Manifest may supply `sha256`; invalid digests fail closed at parse time.
- Package promotion requires SHA-256 when present, then MD5 (NGD integrity).
- Corrupt/wrong-hash partials are discarded and never promoted.
- Tests: optional SHA-256 select/reject + `package_digest_validation_requires_sha256_when_present_and_fails_closed`.

## Rejected for throughput retention

### Parallel MDBX batch reads as default
- Lowered min key threshold to 4,096 and A/B'd `NEO_MDBX_BATCH_READ_THREADS=4` vs `1`.
- Dense mean: control **1,618.1** vs candidate **1,529.1** (−5.5%).
- Roots matched; default remains **serial** (`threads=1`). Threshold 4,096 kept so
  opt-in env var applies to dense-sized batches.

## Dense-window hotspots (baseline 290–300k)

| Stage | Cost |
|-------|-----:|
| MPT window apply | 3.73 s |
| MPT backing commit | 1.66 s |
| MDBX cursor write | 1.11 s |
| MPT queue wait | 1.08 s |
| Finalization backing misses | 808,396 (unique first lookups dominate) |

## Gap to production 1,500–2,000 claim

| Path | Status |
|------|--------|
| Pruning + uncoordinated + tmpfs dense | **In band** (~1.6k multi-run) |
| Coordinated full-state durable high-height | Still ~50–60 bps historically; next hotspot is coordinated work-budget splits + MDBX finalization |
| Live P2P height advance | Environment-dependent |

Next optimization targets (in order):
1. Adaptive coordinated import work-budget (Ledger+StateService co-commit splits).
2. Reduce unique finalization backing misses (deferred hash sets / locality).
3. Dense TX `load_execute` path after StateService costs fall further.
