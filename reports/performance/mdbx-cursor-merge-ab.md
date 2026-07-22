# MDBX Merge-Cursor A/B

Captured 2026-07-16 on the deferred full-state MainNet replay path. The
candidate was enabled with `NEO_MDBX_CURSOR_WRITE_MODE=merge_cursor`; the
durable production default remains the independent-search writer.

Both runs imported authenticated `chain.acc` ranges and verified the same
reference state roots after reopening the database. MPT failures were zero.

## Sparse 1,700,001..1,701,000

The first merge implementation bounded each forward walk at 64 rows but paid
that scan for every sparse content-addressed key:

| writer | import seconds | finalization seconds | cursor-write seconds | entries | root |
| --- | ---: | ---: | ---: | ---: | --- |
| search | 41.891 | 40.002 | 3.226 | 125,751 | `0xad0d861d86d0d10bd78caef754b1ea89cb4263c3c52a126debede3d8b5d309f` |
| bounded merge | 128.299 | 126.283 | 88.877 | 125,751 | same |

This result rejected a per-key bounded scan. The implementation now switches
the remainder of an overlay to the search writer after the first sparse walk
exceeds the bound.

## Adaptive 1,701,001..1,702,000

The adaptive candidate retained exact `CURRENT` updates for dense cursor work,
then switched to independent seeks after detecting sparse keys:

| writer | import seconds | finalization seconds | cursor-write seconds | entries | root |
| --- | ---: | ---: | ---: | ---: | --- |
| search | 22.851 | 21.695 | 0.547 | 95,096 | `0xfbaeb690eb8e2f307bb48d71e51068d99b2a3647260269ca104f2a4d917aea49` |
| adaptive merge | 35.885 | 34.570 | 0.552 | 95,096 | same |

The adaptive path is correctness-preserving but not faster on this sparse
MainNet workload. The rejected production branch and its environment switch
were removed on 2026-07-22; this report is retained as negative evidence.
Artifacts:

- `mdbx-cursor-ab-guarded-1700k-node.log`
- `mdbx-cursor-ab-guarded-1700k.json`
- `mdbx-cursor-ab-search-guarded-1700k-node.log`
- `mdbx-cursor-ab-search-guarded-1700k.json`
- `mdbx-cursor-ab-adaptive-1701k-node.log`
- `mdbx-cursor-ab-adaptive-1701k.json`
- `mdbx-cursor-ab-search-1701k-node.log`
- `mdbx-cursor-ab-search-1701k.json`
