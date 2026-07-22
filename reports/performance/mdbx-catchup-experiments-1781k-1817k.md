# MDBX Catch-Up Experiments

All variants were bounded, root-checked, and kept opt-in or rejected. The
durable production default remains unchanged.

| Variant | Range | Tx blocks/s | Root check | Decision |
|---|---:|---:|---|---|
| `no-meta-sync` | 1,781,001-1,782,000 | 876 | exact at 1,782,000 | rejected, no speedup |
| `safe-no-sync` | 1,782,001-1,783,000 | 1,109 | exact at 1,783,000 | opt-in only; non-durable |
| `coalesce=1` | 1,783,001-1,784,000 | 840 | exact at 1,784,000 | rejected |
| `no_meminit=1` | 1,784,001-1,785,000 | 536 | exact at 1,785,000 | rejected |
| bounded MPT cache, cold | 1,805,001-1,806,000 | 1,021 | exact at 1,806,000 | rejected; zero memory hits |
| bounded MPT cache, warm | 1,806,001-1,807,000 | 760 | exact at 1,807,000 | rejected; zero memory hits |
| pruning clone + `safe-no-sync` | 1,785,001-1,786,000 | 443 | exact at 1,786,000 | not comparable; cold clone |

The cache candidate was removed after the warm run still reported zero
cross-batch MPT memory hits and higher finalization cost. No candidate changes
the durable default or the consensus state-transition path. The rejected MDBX
sync/flag and mapping variants were removed from production configuration on
2026-07-22.

Raw reports and node logs are retained under `reports/performance/` with the
matching range names.
