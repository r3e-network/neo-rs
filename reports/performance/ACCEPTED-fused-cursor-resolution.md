# ACCEPTED: fused MPT deferred-journal resolution at the commit cursor

Date: 2026-07-17. Binary SHA-256 prefix `8fae5107c12721a3` (candidate) vs
the preceding classic-sweep binary (control), same store, same chain.acc,
same strict-shadow config, adjacent 10k-block MainNet windows.

## Mechanism

Deferred full-state journals are exported unresolved
(`MptCache::summarize_deferred_journal`, `neo-trie/src/mpt/cache/operations.rs`)
and carried by `PreparedMptCommit` to the coordinated MDBX commit, which
resolves each node's persisted reference count at the RW cursor
(`commit_raw_overlay_at_cursor`, `neo-state-service/src/storage/mpt_store.rs`)
— one B-tree descent per key instead of a separate sorted read sweep plus a
write pass. Fail-closed guards: unresolved journals reject publication;
non-coordinated publishes reject journals. Equivalence proof:
`fused_cursor_resolution_matches_classic_deferred_finalization_bytes`
(byte-identical persisted rows vs the classic path, references included).

## A/B results

| Window | Mode | Txs | Deferred lookup | Commit windows | Wall | blocks/s |
|---|---|---:|---:|---:|---:|---:|
| 2,007,022-2,017,022 | classic | 5,203 | 144.8 s (214µs/e) | 188.1 s | 368.2 s | 27.2 |
| 2,017,022-2,027,022 | classic | 4,514 | 155.8 s (242µs/e) | 213.5 s | 414.2 s | 24.1 |
| 2,027,022-2,037,022 | classic | 14,715 | 232.9 s (202µs/e) | 305.4 s | 595.7 s | 16.8 |
| 2,037,022-2,047,022 | fused | 17,244 | **0.0 s** | 438.4 s | 533.3 s | 18.8 |
| 2,047,022-2,057,022 | fused | 5,471 | **0.0 s** | 137.4 s | 156.9 s | **63.7** |
| 2,057,022-2,067,022 | fused | 4,276 | **0.0 s** | 92.4 s | 104.4 s | **95.8** |

- Light windows (~4.3-5.5k txs): **2.4-3.0x faster** wall time.
- Heavy windows (~15-17k txs): 1.12x faster; resolution cost moved into the
  commit cursor as designed (438 s commit windows), which is now the top
  hotspot at high tx density.
- Correctness: zero errors, zero overflow skips, zero shadow mismatches;
  seed1 root parity MATCH at 2,047,022 / 2,057,022 / 2,067,022
  (`0x0e88f7ab…`, `0xff1bd0c7…`, `0x794f9799…`).

## Decision

Accepted as the default coordinated deferred full-state path. Next hotspot:
commit-window cost at high density (cursor read-modify-write + overlay visit
bookkeeping, ~21µs/entry), tracked in `docs/optimization-roadmap.md`.

Evidence: `reports/performance/mainnet-shadow-observed-20{3,4,5,6}7022-*.log`,
march log `march-20260717-1044.log`.
