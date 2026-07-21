# Append Persistence v3: 1M-Row Paired Campaign

Date: 2026-07-21 UTC
Scope: persistence-component bakeoff only; this is not an end-to-end Neo node
blocks-per-second claim.

## Workload and controls

- Source: `neo-n3-mainnet-1877001-1887000`
- Seed: `5640001189223798919`
- Prefill: `1,048,576` rows
- Timed campaign: `1,300` represented blocks, `8` commit epochs, `131,072`
  puts, `0` tombstones, `1,118` version hits
- Logical campaign bytes: `29,785,119` value bytes and `34,110,495`
  key-plus-value mutation bytes
- Hardware label: `ultra9-285k-vcpu8`
- Filesystem label: `ext4-shared-vm`
- MDBX durability: `mdbx-safe-durable`
- Append durability: `append-sync-data`
- Read phase: `16,384` point queries and `8,192` sorted-batch keys, five
  rounds each
- Both reports were produced by release binaries from the same dirty worktree
  revision `9711dcb9cd5e8b78d00cac0549c7ebfe8dde3a32`. The append binary was
  rebuilt after the current correctness changes.

Commands:

```text
./target/release/append-persistence-bench --database /tmp/neo-append-1m-20260721-d --output /tmp/neo-append-1m-20260721-d.json --evidence-log /tmp/neo-append-1m-20260721-d.jsonl --scale smoke --hardware-profile ultra9-285k-vcpu8 --filesystem-profile ext4-shared-vm --durability-profile append-sync-data --read-cache-state warm-after-prefill --prefill-batch-entries 32768 --point-queries 16384 --point-rounds 5 --sorted-batch-keys 8192 --sorted-batch-rounds 5 --smoke-prefill-rows 1048576 --smoke-operations 131072 --smoke-blocks 1300 --max-index-memory-mib 1024
NEO_MDBX_SYNC_MODE=durable ./target/release/mdbx-persistence-bench --database /tmp/neo-mdbx-1m-20260721-b --output /tmp/neo-mdbx-1m-20260721-b.json --evidence-log /tmp/neo-mdbx-1m-20260721-b.jsonl --scale smoke --hardware-profile ultra9-285k-vcpu8 --filesystem-profile ext4-shared-vm --durability-profile mdbx-safe-durable --read-cache-state uncontrolled-warm-after-prefill --prefill-batch-entries 32768 --point-queries 16384 --point-rounds 5 --sorted-batch-keys 8192 --sorted-batch-rounds 5 --smoke-prefill-rows 1048576 --smoke-operations 131072 --smoke-blocks 1300
```

## Results

| Metric | Append frames + immutable runs v3 | Durable MDBX | Interpretation |
| --- | ---: | ---: | --- |
| Campaign wall time | `0.201205843 s` | `3.299765163 s` | Append is `16.40x` faster for this isolated component workload |
| Represented blocks/s | `6,461.04` | `393.97` | Component throughput only |
| Process physical writes | `53,624,832 B` | `4,235,558,912 B` | Same logical workload |
| Write amplification vs values | `1.800x` | `142.204x` | Append is `78.99x` lower |
| Write amplification vs mutations | `1.572x` | `124.172x` | Append is `79.04x` lower |
| Append compaction | `4` cycles, `36` runs merged | n/a | Derived indexes compacted and reclaimed |
| Reopen validation | `40` frames, `8` runs, `1,179,648` index entries | `8` epoch coverage | Both reopen checks passed |
| Reopen sampled digest | `73c932fc04bb1e68dadf20dce6ec302edb0cdd4b2dc661fccaef063eaf728a44` | same | Exact equality |

Append peak sampled RSS during the campaign was `62,808,064 B`; durable MDBX
peak sampled RSS was `729,784,320 B`. The append campaign used `66` write
syscalls and its largest measured stage was pack sync (`27.47 ms`), with pack
sync and index build overlapped.

## Evidence files

- [append JSON](append-persistence-v3-1m-20260721-append.json)
- [append evidence log](append-persistence-v3-1m-20260721-append.jsonl)
- [MDBX JSON](append-persistence-v3-1m-20260721-mdbx.json)
- [MDBX evidence log](append-persistence-v3-1m-20260721-mdbx.jsonl)

## Limits and next gate

This result validates a persistence backend direction and the exact workload
contract. It does not prove MainNet execution correctness, state-root parity,
network sync, or the requested `1,500-2,000` end-to-end blocks/s. The append
prototype still requires the OpenSpec shadow, crash, retention, authoritative
promotion, and ordered-pipeline gates before it can replace the canonical
path. MainNet node throughput must continue to be reported separately with
StateRoot enabled and disabled where those modes are measured.
