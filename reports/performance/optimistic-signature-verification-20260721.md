# Optimistic Header Signature Preverification: MainNet A/B

## Outcome

Commit `2c6486e17ced1f71be04eaa76578d200797e3c91` improves the mean
StateRoot-enabled verified-import rate from **255.04 to 346.63 blocks/s**
across two order-reversed pairs, a **35.91%** increase. The individual pairs
were `260.68 -> 357.93` (+37.31%) and `249.39 -> 335.32` (+34.45%).

This is a valid optimization, but it does not meet the required 2,000
blocks/s. The measured mean remains **5.77x below** that target.

## Safety model

The workers do not authorize blocks, read state, or execute a second VM. They
precompute exact secp256r1 outcomes for recognized standard header witnesses.
The ordered canonical lane binds the cache to the exact header hash, height,
network, and complete witness, checks the parent/timestamp/primary index and
previous `NextConsensus`, and then executes the complete witness using the
workspace `neo-vm`. Unsupported input, stale results, queue pressure, worker
failure, and cache misses use the ordinary synchronous path. The feature is
disabled by default.

## Campaign

- Host: Linux 6.17, 8 visible CPUs, Intel Core Ultra 9 285K, 62 GiB RAM, ext4.
- Window: MainNet `3,452,023..=3,457,022` (5,000 blocks).
- Composition: 4,251 empty blocks, 749 transaction blocks, 877 transactions.
- Both arms: `--enable-stateroot --verify-import-chain`.
- Enabled arm: 4 workers, queue capacity 32, ordered window 36.
- Node SHA-256: `898d72eb0c29dc12c51bedc9d8414be6e2948ee2336b69781ca90cf062a0780d`.
- MDBX input SHA-256: `b2be935ef66c7330b1a005d3532a86e4d1430d5e2a1a38a25acb56ddbbda9e1f`.
- Archive SHA-256: `6043a5c91735087bfb4dda33a2755603f58b6ce3706104510e726ea9aa78b1c0`.
- Harness SHA-256: `e55531c6726dac00f6541d3d3be7bf709c2936c7b49cd23814be29a2ce2d731d`.

Each arm used a fresh physical MDBX and authoritative-pack clone. The first
pair ran disabled then enabled; the second ran enabled then disabled. BPS is
the node's measured chain-import interval. Full process wall time was roughly
115-125 seconds per arm because pack open/recovery work occurs outside that
interval; the mean full-process time did not materially improve.

| Order | Disabled BPS | Enabled BPS | Delta | Disabled finalization/store | Enabled finalization/store |
|---|---:|---:|---:|---:|---:|
| off -> on | 260.68 | 357.93 | +37.31% | 10.753 s | 10.299 s |
| on -> off | 249.39 | 335.32 | +34.45% | 11.948 s | 11.181 s |
| mean | **255.04** | **346.63** | **+35.91%** | **11.350 s** | **10.740 s** |

The enabled arms each submitted and completed 4,999 header jobs. Each produced
33,188 exact ECDSA outcomes and the canonical NeoVM path consumed all 33,188:
zero cache misses, invalid jobs, cancellations, panics, unavailable workers, or
closed queues. Queue-full backpressure occurred 572 and 510 times and recovered
through the bounded ordered window.

## Correctness gates

Every arm reached ledger and StateService height `3,457,022` and StateRoot
`0x10c5f09f30c7ba565b522b56c4194bcf88bd7ee79484344e6dea556adb8fd5f6`.
Every arm also produced the same authoritative marker:

```text
epoch=297 frame_end=56517773366 block=3457022
root=0xf6d58fdb6a55ea6d4e348494e77ebd88cf4b19c4562b525b56bac7309ff0c510
payload_sha256=0xbcbabe26106cc0506fa3cfdade4240c70a472671ffe6f1756d869e336d55bc5f
```

Both output packs passed authority verification plus full payload and index
scrubs. The implementation also passed 274 `neo-blockchain` tests, the full
`neo-execution` and `neo-node` suites, strict all-target Clippy, layer-boundary
tests, strict OpenSpec validation, formatting, and `git diff --check`.

Raw evidence is retained at:

- `/tmp/neo-signature-cache-ab-20260721T120107Z-evidence`
- `/tmp/neo-signature-cache-ab-20260721T121451Z-evidence`

## Remaining bottleneck

The optimization removes roughly 4.3 seconds of previously unclassified
ordered signature latency, but finalization/store still consumes 10.3-11.95
seconds per 5,000 blocks. Transaction VM execution is about 2.2 seconds. The
next high-value work is therefore the already identified batched finalization
architecture: prepare ledger archive, state-pack frames, and indexes in
parallel, merge sorted changes across blocks, and publish them through one
ordered durable fence with crash/reopen proof. More signature tuning cannot
close the remaining 5.77x gap.
