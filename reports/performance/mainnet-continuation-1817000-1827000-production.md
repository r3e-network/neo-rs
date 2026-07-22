# MainNet Continuation 1,817,000-1,827,000

Captured 2026-07-16 with the rebuilt production release binary, durable MDBX,
deferred full-state finalization, and
`NEO_MDBX_WRITE_INTENT_READ_THREADS=16`.

- Ledger and StateService heights: `1,827,000`.
- Local/reference root: `0xec4e3d923618dbf803c488e6257c4104a216c6dd2d572b38eca966d59c472aba`.
- `seed1.neo.org:10332 getstateroot(1827000)` matched after reopen.
- Advanced blocks: `10,000`.
- Import elapsed time: `44.089 s` (`226.81 blocks/s`).
- Transaction-bearing throughput: `813.10 blocks/s` across 2,718 blocks and 4,324 transactions.
- Finalization/store-commit time: `40.550 s`.
- MPT failures: `0`.

MDBX commit work accounted for `35.01 s`; cursor writes accounted for `10.03 s`.
The production speed gate remains intentionally failed.
