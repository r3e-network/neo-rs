# MainNet Continuation 1,837,000-1,847,000

Captured 2026-07-16 with release node
`7cc0043e43ba18a8f1333388738d5f4a641ad5a8a67a568931cd94c8638e85ee`,
durable MDBX, deferred full-state finalization, and
`NEO_MDBX_WRITE_INTENT_READ_THREADS=16`.

- Ledger and StateService heights after reopen: `1,847,000`.
- Local/reference root: `0x52f4d659ee7941abc2ca90d80ffdeb3712d92e08bab14bc8ee5623516dc6f8c0`.
- `seed1.neo.org:10332` and `seed2.neo.org:10332 getstateroot(1847000)`
  matched after reopen.
- Advanced blocks: `10,000`.
- Import elapsed time: `49.772 s` (`200.92 blocks/s`).
- Transaction-bearing throughput: `662.49 blocks/s` across 2,861 blocks and
  5,412 transactions.
- Empty-block throughput: `38,555.53 blocks/s` across 7,139 blocks.
- Finalization/store-commit time: `45.255 s`.
- MPT failures: `0`.

Live Prometheus capture succeeded for 10 polls with 223 metrics per poll,
covering native persistence from height 1,839,182 through 1,846,998. Three
polls failed only during endpoint startup or shutdown. MDBX commit work
accounted for `33.92 s`, including `27.31 s` in commit fences and `6.14 s` in
cursor writes. Deferred backing lookups accounted for `9.11 s`, trie commits
for `9.58 s`, and VM transaction execution for `3.37 s`; these nested stage
totals are not additive. GNU time recorded `32.06 GiB` peak RSS, 108,950 major
page faults, 16,485,640 filesystem input units, and 132,985,608 filesystem
output units.

The runner reported `sync-speed-too-slow`: transaction-bearing throughput was
`837.51 blocks/s` below the `1,500 blocks/s` floor. Correctness gates passed:
the release node exited successfully at the target, both references matched,
fresh processes reopened both coordinated MDBX namespaces, 1,000 leaves were
traversed from the persisted root, and a sampled leaf resolved through that
root. OpenSpec task 4.5 remains incomplete pending replay to MainNet tip and
final root agreement.
