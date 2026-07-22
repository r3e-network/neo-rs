# Adaptive Ordered MPT Backing Reads

Captured 2026-07-16 from the release binary built from the current worktree.

The deferred StateService finalization path emits unique MPT node keys in
ascending hash order. MDBX now exposes `try_get_many_bytes_sorted`; its MPT
caller uses a single forward cursor while adjacent keys remain local and
falls back to the existing seek-per-key cursor path after 64 forward steps for
one key. This prevents sparse absent-key workloads from scanning unrelated
MPT rows while retaining a fast path for clustered reads.

## Correctness

- Input: MainNet `chain.0.acc`, heights 100,001 through 300,000.
- State mode: `full_state=false`, asynchronous StateService, tmpfs MDBX.
- Ledger and StateService both reached height 300,000.
- MPT failures: 0.
- State root: `0xf0e121ac28b2de37e7e0321b0e0ef926f8b1ff9e85ee6f03c0c6c242a5f31088`.
- Independent `seed1.neo.org:10332 getstateroot(300000)` reference: matched.

## Performance

The final 10,000-block window (heights 290,001-300,000) reported:

| Measure | Result |
|---|---:|
| Blocks/s | 1,764.43 |
| Transaction-bearing blocks/s | 1,987.90 |
| MPT apply | 3.181 s |
| MPT backing commit | 1.277 s |
| Import finalization | 3.237 s |
| Finalization store commit | 1.244 s |
| Finalization backing misses | 805,945 |

The full 200,000-block import averaged 12,362.56 blocks/s; the dense-window
result is the relevant comparison for the production target. The run was
tmpfs and CPU-pinned, so it is not a durable-disk or live-P2P release proof.

The raw log is `ordered-mpt-ab/candidate-node-h100-300k.log`.

SHA-256: `f80cd8401cf2b6844d9c2d0b7f07c76ebbec6e0bcaa267632b09967cdbcf6430`.
