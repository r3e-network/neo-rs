# Authoritative Pack One-Copy Publication A/B

Status: **accepted** as a state-pack publication optimization. The measured
MainNet result is **343.46 blocks/s**, still far below the 1,500-2,000 blocks/s
target.

## Change

Baseline publication first cloned every materialized MPT value into an owned
`PackOperation`, then copied it again into the final append-frame payload. The
candidate preallocates the exact payload from existing overlay counters, copies
borrowed values directly into that payload, and reuses the already sorted key
order when building the run. The owned API and on-disk frame/index formats are
unchanged.

Baseline revision: `db059b62972e7b85b652f5f67c4ec3d742ae43ea`
(`neo-node` SHA-256 `6b1c80358e092d06c16bede478b61ca85ea01a1b2dfb27879ecc7addbf484499`).

Candidate revision: `e5445029f648b91aacae05b6ebd019bb5a01ea57`
(`neo-node` SHA-256 `cbc59cd299c48185ab92b2939c5d31e408fdb1e4a68259561ad868829affad30`).

## Paired MainNet Result

Both sides replayed MainNet blocks `3,382,023..3,387,022` from the same logical
authority marker, on the same host and ext4 filesystem. Each side ran `sync`
then `POSIX_FADV_DONTNEED` over its pack, MDBX, and archive files. Configuration,
durability, execution shadow, and timing boundaries were otherwise identical.

| Metric | Baseline | Candidate | Signed delta |
|---|---:|---:|---:|
| Overall blocks/s | 318.3612 | 343.4567 | **+7.8827%** |
| Transaction-bearing blocks/s | 286.1418 | 286.6930 | **+0.1926%** |
| Driver blocks/s | 317.7630 | 342.4022 | **+7.7540%** |
| Empty-block path blocks/s | 30,450.41 | 32,202.36 | +5.7535% |
| Import elapsed | 15.7054 s | 14.5579 s | -7.3067% |
| Finalization/store | 8.8087 s | 7.6795 s | **-12.8187%** |
| MPT apply | 8.8057 s | 7.6758 s | -12.8316% |
| MPT backing publication | 5.4647 s | 4.8564 s | -11.1326% |
| VM execute | 5.3686 s | 5.4925 s | +2.3070% |
| MDBX commit window | 2.7886 s | 3.0035 s | +7.7056% |

Each side imported 5,000 blocks: 1,938 transaction-bearing blocks, 3,062 empty
blocks, and 2,916 transactions. Overall blocks/s uses `5000 / elapsed_seconds`;
that boundary includes execution, state finalization, and durable canonical
publication, while excluding startup, pack reopen, archive read, and archive
validation. Transaction-bearing blocks/s uses the node's narrower
`1938 / transaction_block_import_seconds` path metric.

Corpus SHA-256:
`6043a5c91735087bfb4dda33a2755603f58b6ce3706104510e726ea9aa78b1c0`
(`chain.0.acc`, 10,031,375,631 bytes). Base checkpoint SHA-256:
`cb648e630968c35c36fb89119416cc5be534e3caf0d5ab9d21991b33121f13f6`.
The exact starting marker was block `3,382,022`, epoch `263`, frame end
`55,392,359,055`, internal root
`d5f0b14d87ceaed5a4219d783f4ed592101438488dea6178684ac08a5d4ef939`,
and payload SHA-256
`f499135f867280b9e49278b0ba3a077520fd10040747734d1d21650776ea896d`.

## Direct Publication A/B

The identical durable component campaign used a 1,048,576-row prefill,
1,007,960 operations, 10,000 represented blocks, and eight commits. Three runs
per side produced:

| Metric | Owned mean | Borrowed mean | Signed delta |
|---|---:|---:|---:|
| Represented blocks/s | 7,938.49 | 8,868.91 | **+11.7204%** |
| Campaign wall | 1.2597 s | 1.1276 s | -10.4881% |
| CPU time | 1.2133 s | 1.0800 s | -10.9890% |
| Peak RSS | 219.40 MiB | 190.92 MiB | -12.9823% |

All six campaign digests were
`0be95f940753dc742d18af56e010924ce9e4c17183f0b7f2c176cbedf19609cf`,
and all reopen digests matched.

## Correctness Gates

- Baseline and candidate ended at public StateRoot
  `0xbad6c5bdbad2ce21231b7192556ef1fa7f4e94bcdd73adfb1c0a81929fc21b2c`.
- Both reported zero MPT apply failures and zero deferred lookup errors.
- Reopen storage preflight passed for the candidate.
- Both authority verifiers matched winner digest `57cfd7e7...d6fd81`, lookup
  digest `9f101f89...8ed2b3`, and complete frame-reference digest
  `9c95632c...d606ad` across 267 frames.
- Borrowed and owned encoders produce byte-identical frames and run indexes for
  sorted/unsorted input, duplicates, tombstones, empty values, and nonzero frame
  offsets; invalid counts and sizes fail before durable writes.
- `neo-state-packs` (68 passed, 1 ignored), `neo-state-service` (129 passed), and
  the complete `neo-node` suite (410 + 14 + 4 + 1 passed) succeeded. Strict
  all-target Clippy, formatting, diff checks, and strict OpenSpec validation
  passed.

The MainNet pair is one run per side. Its deferred lookup became faster while
MDBX commit time became slower, and post-run derived index topology differed;
therefore `+7.8827%` is the exact observed paired result, not a universal speed
forecast. The repeated direct publication A/B is the cleaner attribution for
the allocation/copy change.
