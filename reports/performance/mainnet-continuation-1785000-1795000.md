# MainNet Continuation 1,785,000-1,795,000

Captured 2026-07-16 on the retained full-state MDBX database with the durable
commit path and sixteen write-intent readers.

- Ledger and StateService heights: `1,795,000`.
- Local/reference root: `0x0b55015615e63b8f0a545cb94487ff4bcbf52a5cde61e2b8c60c95ddb22fd030`.
- `seed1.neo.org:10332 getstateroot(1795000)` matched after reopen.
- Advanced blocks: `10,000`.
- Import elapsed time: `36.183 s` (`276.37 blocks/s`).
- Transaction-bearing throughput: `859.74 blocks/s` across 2,631 blocks and 3,938 transactions.
- Finalization/store-commit time: `32.951 s`.
- MPT failures: `0`.

Durable MPT/MDBX finalization remains the limiting stage; VM execution for the
3,938 transactions took approximately `2.18 s` in aggregate.
