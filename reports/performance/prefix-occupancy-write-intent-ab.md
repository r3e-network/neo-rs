# Prefix Occupancy Write-Intent A/B

Captured 2026-07-16 while profiling the deferred full-state MPT finalizer.

The occupancy bitmap remains useful for ordinary read-only batches, but a
write-coupled MPT finalization lookup has a different locality requirement: a
missing-key probe warms MDBX B-tree pages that the following overlay writer
will visit. The raw storage trait now exposes that intent explicitly through
`try_get_many_bytes_sorted_for_write`; MDBX bypasses the bitmap only for that
method.

## Evidence

| Run | Range | Index | Transaction-bearing BPS | Cursor write | MDBX commit | Root |
|---|---:|---|---:|---:|---:|---|
| Earlier valid-index candidate | 811,001-812,000 | filtered | 640.8 | 8.51 s | 1.96 s | `0xacd968...598fac` |
| Write-intent candidate | 812,001-813,000 | filtered for read-only paths, bypassed for MPT write intent | 684.0 | 1.22 s | 2.09 s | `0xb0e89a...53ab07` |

The write-intent run reached Ledger and StateService height 813,000 with zero
MPT failures. Its root matched `seed1.neo.org:10332 getstateroot(813000)`.
The retained structured log is
`fullstate-profile-h812-813-writeintent-prefix-node.log` (SHA-256
`4030c7679503e1f985aab7e0ab8077cc7b89f5501318e1fa5da025ccde3afd39`).

The earlier filtered run synthesized most negative results from the bitmap and
made the subsequent cursor traversal cold. This is a cache-locality regression,
not a correctness failure. The occupancy index stays opt-in and trusted-startup
mode was removed on 2026-07-22 because unregistered writers cannot safely extend
the artifact's transaction coverage.
