# MainNet writer-lease strict-shadow replay: 3,267,023..3,277,022

Date: 2026-07-18

Host: VMware guest, 8 vCPUs reported as Intel Core Ultra 9 285K, 62 GiB RAM,
Linux 6.17.0-35-generic, and durable MDBX on ext4 (`/dev/sda2`,
`rw,relatime`).

## Result

- Imported 10,000 blocks containing 4,332 transactions.
- Final Ledger and StateService height: 3,277,022.
- Local state root:
  `0x6798c3dd0ab647bc77148036c35fbab7557a64e2b600b03268fcf2b119e96a62`.
- `seed1.neo.org` and `seed2.neo.org` returned the same root.
- Strict ordinary-authoritative specialization Shadow completed with zero
  artifact mismatches and zero infrastructure failures.
- The existing 7.8 GiB delta pack reopened under the new kernel-held writer
  lease, reconciled marker epoch 338, and published four new cold-first frames
  without prepare, marker, activation, recovery, or compaction errors.

Evidence:

- Machine report: `mainnet-shadow-observed-3267022-3277022.json`
- Node log: `mainnet-shadow-observed-3267022-3277022-node.log`
- Release `neo-node` SHA-256:
  `af9b785fb1184e1ef7f3e2a891a681846cd15269b9c6494c9daabacc20dd13d5`

## Throughput and hotspots

| Work/stage | Count or time | Rate/share |
| --- | ---: | ---: |
| All blocks | 10,000 in 225.826 s | 44.28 blocks/s |
| Transaction blocks | 2,960 in 8.256 s | 358.52 blocks/s |
| Empty blocks | 7,040 in 0.210 s | 33,475.65 blocks/s |
| NeoVM transaction execute | 6.396 s | 2.83% of import wall |
| Engine construction | 0.057 s | 0.03% of import wall |
| Finalization/store commit | 217.345 s | 96.24% of import wall |
| MDBX coordinated commit | 213.774 s | 94.66% of import wall |
| MDBX cursor resolve | 181.088 s | 80.19% of import wall |
| MDBX durable commit | 29.317 s | 12.98% of import wall |

Four coordinated commits applied 645,174 entries and 148,916,479 value
bytes. StateService produced 602,298 node puts and no node deletes.

This window strengthens the architecture decision. Removing cursor resolution
alone would leave 44.739 seconds, or 223.52 blocks/s. Also removing the current
durable-commit time would leave 15.421 seconds, or 648.46 blocks/s. Assuming
NeoVM execution were free as well would still leave 9.025 seconds, or 1,108.00
blocks/s. Reaching 2,000 blocks/s therefore requires the combined authoritative
pack cutover, lower residual publication overhead, bounded stage overlap, and
validated parallel execution; an engine pool or object pool alone cannot do it.

## Pack checkpoint

After shutdown, the canonical marker selected epoch 342 (343 total frames).
Its final commit window covered blocks 3,274,313..3,277,022 with 133,042 node
operations and 30,601,084 put-value bytes. Reversing the marker's internal
UInt256 bytes yields the displayed public state root above.

A bounded verifier walked and compared 1,000 MDBX node keys. It found 142 pack
hits, all byte-identical to MDBX; 858 keys predated shadow activation and there
were zero mismatches. Verification took 0.10 seconds with 149,736 KiB maximum
RSS.

This remains delta-shadow evidence, not authority. The pack has a production
single-writer lease and correct two-phase publication, but it is not complete
from genesis and still lacks the complete footer/segment identity and migration
gates required before MDBX `0xf0` reads or writes can be removed.
