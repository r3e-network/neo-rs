# MainNet Continuation 1,827,000-1,837,000

Captured 2026-07-16 with the rebuilt production release binary, durable MDBX,
deferred full-state finalization, and
`NEO_MDBX_WRITE_INTENT_READ_THREADS=16`.

- Ledger and StateService heights after reopen: `1,837,000`.
- Local/reference root: `0x9e2a33d44de098b7728b4c6356cb457aabf725bfd4a0427857f37df3fc2217ab`.
- `seed1.neo.org:10332` and `seed2.neo.org:10332 getstateroot(1837000)`
  matched after reopen.
- Advanced blocks: `10,000`.
- Import elapsed time: `58.238 s` (`171.71 blocks/s`).
- Transaction-bearing throughput: `633.78 blocks/s` across 3,027 blocks and
  5,721 transactions.
- Empty-block throughput: `37,097.76 blocks/s` across 6,973 blocks.
- Finalization/store-commit time: `53.250 s`.
- MPT failures: `0`.

MDBX commit work accounted for `37.26 s`, including `27.21 s` in commit
fences and `9.52 s` in cursor writes. Deferred backing lookups accounted for
`13.38 s`, trie commits for `13.84 s`, and VM transaction execution for
`3.51 s`; these concurrent/nested stage totals are not additive. GNU time
recorded `29.77 GiB` peak RSS, 186,605 major page faults, 22,212,416 filesystem
input units, and 132,050,208 filesystem output units.

The configured Prometheus endpoint was not exposed during archive import, so
all 14 polls recorded connection refusal and the bounded runner reported
`metrics-unavailable`. Structured importer/MPT/MDBX counters were still
captured in the node log. The transaction-bearing production speed gate also
remains failed by `866.22 blocks/s` against the `1,500 blocks/s` floor.

Fresh-process integrity probes reopened both coordinated MDBX namespaces,
matched Ledger and StateService heights, decoded the persisted StateRoot,
traversed 1,000 leaves from that root, and resolved a sampled leaf directly
through the reopened trie. This advances the retained replay checkpoint but
does not complete OpenSpec task 4.5, which still requires replay to MainNet
tip and final root agreement.
