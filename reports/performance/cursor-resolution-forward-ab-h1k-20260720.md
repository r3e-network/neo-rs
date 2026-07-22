# Deferred cursor-resolution A/B: MainNet genesis window

This experiment compares the new ordered deferred-journal cursor against the
established independent MDBX `set_range` path. Each A/B pair used one identical
release binary, the MainNet configuration, complete `chain.0.acc`, StateRoot
enabled, and fresh MDBX stores. The optimized mode was explicitly enabled with
`NEO_MDBX_CURSOR_RESOLUTION_MODE=merge`; the default remains `search`.

## Correctness gate

| mode | final Ledger/MPT height | StateService root | MPT failures |
| --- | ---: | --- | ---: |
| search | 1,000 | `0x60d823c9201730107590ad98684f0369c65b6f0a050a924244f078f0f345e27e` | 0 |
| merge | 1,000 | `0x60d823c9201730107590ad98684f0369c65b6f0a050a924244f078f0f345e27e` | 0 |

The roots and heights match exactly. The full structured reports are
`cursor-resolution-search-h1k.json`, `cursor-resolution-merge-h1k.json`, plus
the two independent repeats with `-rerun` and `-repeat3` suffixes.
The final release verification is recorded with the `-final` suffix.

## Measured result

| run | mode | import finalization/store | cursor-resolve | tx-blocks/s | observed wall BPS |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | search | 20.180 ms | 5.742 ms | 4,763.64 | 948.23 |
| 1 | merge | 19.703 ms | 4.610 ms | 5,077.70 | 947.47 |
| 2 | search | 23.795 ms | 7.390 ms | 5,032.32 | 948.23 |
| 2 | merge | 20.258 ms | 4.953 ms | 5,322.82 | 947.49 |
| 3 | search | 21.165 ms | 6.178 ms | 5,312.72 | 947.87 |
| 3 | merge | 19.896 ms | 4.942 ms | 4,713.49 | 947.49 |
| 4 | search | 19.958 ms | 5.479 ms | 5,420.73 | 948.14 |
| 4 | merge | 19.220 ms | 5.117 ms | 2,937.33 | 947.72 |

Across the four fresh-store pairs, the ordered cursor reduced the measured
cursor-resolve stage in every run (6.6%-33.0%, mean 19.8%). End-to-end
observed wall time is dominated by the one-second height polling interval and
is not a useful speed signal here. The transaction-block rate is based on only
two transaction blocks, so it is also not a production throughput claim.

The existing high-height profile remains the binding production evidence:
deferred finalization and durable MDBX commit dominate the wall time, while
content-addressed keys are sparse. The optimization therefore stays opt-in
until a high-height A/B demonstrates a repeatable finalization reduction and
exact reopen parity.
