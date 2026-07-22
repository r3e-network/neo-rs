# MDBX Reader Repeat A/B

Captured 2026-07-16 from the restore-verified MainNet checkpoint at height
811,000, replaying the identical 1,000-block chain.acc window through height
812,000. Both runs used deferred full-state finalization, the default
`NEO_COORDINATED_IMPORT_CHANGE_BUDGET=8192`, durable MDBX commits, and the
same release binaries for the first three rows. The final row used a rebuilt
release containing the write-intent-only selector. The first three rows changed the global
`NEO_MDBX_BATCH_READ_THREADS`; the final row uses only the write-intent-specific
override introduced by this change.

## Correctness

- All four runs completed 1,000 blocks with zero MPT failures.
- Both produced the official height-812,000 root:
  `0xacd96890371c7c1df925cb172d2df7b5e87731b26f6bff7fa9a8284ed7598fac`.
- The root was also returned by `seed1.neo.org:10332 getstateroot(812000)`.
- Ledger and StateService probes both reported height `812000`.

## Performance

| MDBX readers | Import BPS | Transaction-bearing BPS | Finalization | MDBX commit |
|---:|---:|---:|---:|---:|
| 8 | 155.14 | 744.28 | 5.890 s | 2.414 s |
| 16 | 182.30 | 904.42 | 5.025 s | 2.191 s |
| 16 (repeat 2) | 172.64 | 686.49 | 5.189 s | 2.315 s |
| write-intent 16 | 183.18 | 781.21 | 4.927 s | 2.202 s |

The first fresh 16-reader run improved transaction-bearing throughput by 21.5%
and full-window import throughput by 17.5% over the matched 8-reader control.
The second fresh 16-reader run was 7.8% slower overall and 7.8% slower on
transaction-bearing blocks, despite identical inputs. Together with the older
16-reader run, this variance rejects changing the production default based on
reader count alone. The setting remains an opt-in candidate until profiling on
a quiet, pinned host separates MDBX reader effects from storage and scheduler
noise.

The write-intent-only run left the global reader setting unset and used
`NEO_MDBX_WRITE_INTENT_READ_THREADS=16`; its deferred lookup measured 1.600 s
and its MPT failure count was zero. The run demonstrates the narrower tuning
surface and is not a production-speed claim. Later high-height normalized
evidence rejected this approach, and both environment switches were removed
on 2026-07-22.

Artifacts:

- 8-reader node log: `fullstate-profile-h811-812-repeat8-node.log`, SHA-256
  `4ca280378a954022045420c5494b052869ef63956b52fd0f002615c66cc718f7`.
- 16-reader node log: `fullstate-profile-h811-812-repeat16-node.log`,
  SHA-256 `ae7b0f8c6b15a494aca5123e43f0243f380a0828daaa4019125dc7efbb70ad18`.
- 16-reader repeat node log: `fullstate-profile-h811-812-repeat16c-node.log`,
  SHA-256 `6782880cce7642a61ee7f6aba9df9abfa8206cedbf7944507a79ce2f4666c08c`.
- Write-intent-only node log: `fullstate-profile-h811-812-writeintent16-node.log`,
  SHA-256 `8170b91c206c73798b73721db47ca8dc0e81f6778e1c40c894c89a578958705a`.
- Release `neo-node` used for the write-intent-only run, SHA-256
  `d78c4a4dc5c4e13adc5a2e54eaae535a73df3c8a55272f4f508f9623ba9458da`.
- Release `neo-db-probe` used for the write-intent-only run, SHA-256
  `4a509c358a76a45800ebe167e3394cc9fa6872b2f9908441819a799bfaa1574c`.

This is durable-disk profiling evidence, not a staged/full MainNet replay
completion proof and not evidence that the 1,500-2,000 blocks/s production
target has been met.
