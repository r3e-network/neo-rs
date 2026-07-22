# Append-frame persistence prototype: one-million-row comparison

Date: 2026-07-17

## Scope

This is a benchmark-only append-frame and immutable sorted-index-run prototype.
It is not wired into the node and is not an authoritative storage backend. The
comparison uses the same deterministic workload and runner controls as
`mdbx-persistence-1m-20260717.json`:

- 1,048,576 prefilled rows;
- 131,072 durable campaign mutations representing 1,300 blocks;
- eight campaign durability fences;
- 16,384 point queries for five rounds;
- 8,192-key sorted batches for five rounds;
- Intel Core Ultra 9 285K VM with eight visible vCPUs;
- ext4 on the shared VMware virtual disk.

The release append binary SHA-256 was
`ca2b1b98c4d5656bc8f20e2baa46b65bdb8305d6cbace20ad0ae2b0a362eec70`.

## Correctness evidence

- Prefill operation SHA-256 matched MDBX:
  `7d59a88eed8158dbf6a2f12eb16193fa92e036c20cd3dfe18ab9d8e13a6537ba`.
- Campaign operation SHA-256 matched MDBX:
  `320749535a057994ede6bf31b0f97846c3cc5d1d8a800aceec0b2d396f07eac1`.
- The expected and reopened 32,798-query digests matched each other and MDBX:
  `73c932fc04bb1e68dadf20dce6ec302edb0cdd4b2dc661fccaef063eaf728a44`.
- Reopen validated 40 checksummed frames, 40 checksummed sorted runs, and
  1,179,648 index records.
- Focused tests cover newest-row semantics within one run, newer-run
  replacement, tombstones, checksums, bounded index memory, and atomic report
  publication.

## Results

| Metric | Durable MDBX | Append prototype | Ratio |
|---|---:|---:|---:|
| Campaign wall time | 3.187 s | 0.108 s | 29.48x faster |
| Represented blocks/s | 409.08 | 12,024.74 | 29.39x |
| Process physical writes | 4,235.56 MB | 41.39 MB | 102.34x less |
| Write amplification vs values | 142.20x | 1.39x | 102.34x less |
| Point-read p50 | 282 ns | 3,269 ns | 11.59x slower |
| Point-read p99 | 1,414 ns | 21,100 ns | 14.92x slower |
| Sorted-batch p50 per key | 4 ns | 2,879 ns | 719.75x slower |
| Reopen | 0.884 ms | 154.576 ms | 174.88x slower |

The append campaign spent 5.64 ms writing frame bytes, 17.96 ms syncing the
pack, 1.45 ms writing index runs, 8.57 ms syncing index runs, and 1.47 ms on
directory sync. Final layout was 312,895,575 pack bytes plus 58,984,960 index
bytes. Decoded index entries retained an estimated 66,060,288 bytes under the
1 GiB hard bound.

## Decision

The storage shape passes the task 1.3 bakeoff gate: it reduces physical write
amplification by more than an order of magnitude and exceeds the named
1,500-2,000 blocks/s persistence target on this synthetic high-height shape.
It does not pass a production-backend gate. Newest-run linear search makes
reads and restart far slower than MDBX, and the prototype has no filters,
leveled compaction, memory-mapped top-level index, snapshot leases, recovery
marker, crash injection, or MainNet shadow publication.

The next node-pack work must preserve the append write result while replacing
linear run search with a bounded verified lookup hierarchy. MainNet end-to-end
throughput remains unproven until the format is integrated in shadow mode and
exact node bytes, roots, proofs, reopen, crash, and unwind behavior match MDBX.

Machine-readable evidence:

- `append-persistence-1m-comparable-20260717.json`
- `append-persistence-1m-comparable-20260717.jsonl`
- `mdbx-persistence-1m-20260717.json`
