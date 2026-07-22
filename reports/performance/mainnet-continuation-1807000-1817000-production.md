# MainNet Continuation 1,807,000-1,817,000

Captured 2026-07-16 with the rebuilt production release binary, durable MDBX,
deferred full-state finalization, and
`NEO_MDBX_WRITE_INTENT_READ_THREADS=16`.

- Ledger and StateService heights: `1,817,000`.
- Local/reference root: `0x3506b8c2de355c5a3fc356bc45ca47974f1739362ef336b6226032151995728d`.
- `seed1.neo.org:10332 getstateroot(1817000)` matched after reopen.
- Advanced blocks: `10,000`.
- Import elapsed time: `51.723 s` (`193.34 blocks/s`).
- Transaction-bearing throughput: `637.73 blocks/s` across 2,307 blocks and 3,557 transactions.
- Finalization/store-commit time: `47.913 s`.
- MPT failures: `0`.

The stage profile measured approximately `37.12 s` in MDBX commit work,
`14.26 s` in cursor writes, `8.03 s` in deferred lookups, and `2.45 s` in VM
execution. The 1,500-2,000 production band remains unsatisfied.
