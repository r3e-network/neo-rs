# MainNet Replay Evidence

What MainNet replay has and has not established.

An earlier version of this document reported a single "PASSED — 100% Correct"
full-archive StateRoot validation. That framing did not survive a check against
its own source data: the run it cited was launched with `--stateroot false` and
computed no state roots at all. The claim has since been re-established from a
different run, with the comparison actually performed. This document records the
comparison, not a summary of it.

## Summary

A StateRoot-enabled replay of MainNet from genesis to the archive tip at
**11,492,708** exists and its state roots match the C# reference at **11,497
sampled heights** across the whole range, with zero mismatches. Live P2P sync
past the archive tip is validated per-block against the same reference.

Evidence files live in `reports/stateroot-parity/`; read that directory's README
for the method and for what strided sampling does and does not prove.

## The replay

| Property | Value |
|---|---|
| Range | genesis → 11,492,708 (archive tip) |
| StateRoot | **enabled** — all 229 node invocations logged `stateroot_enabled: true`, zero logged false |
| MPT apply failures | 0, at every slice, from h=10,000 through h=11,492,708 |
| Final MPT apply height | 11,492,708 |
| Built | 2026-07-24, 06:40 → 11:06 UTC, in ~50,000-block slices |
| Mode | pruning (`full_state = false`, `track_during_catchup = true`) |
| Database | `data/neo-v3101-staged-replay/pruning-stateroot-chain`, 22 GB |

`state_service_mpt_apply_attempts` tracked `imported` 1:1 for every slice. That
pairing is the check that separates a real StateRoot run from a no-op: a run
without `--enable-stateroot` reports `0 / 0` for attempts and failures, and a
reader who looks only at the failure count sees a zero that means nothing.

## The comparison

`reports/stateroot-parity/pruning-stateroot-chain-vs-seeds-stride1000-20260726.json`

| Metric | Value |
|---|---:|
| Reference heights compared | 11,497 |
| Matched | **11,497** |
| Mismatched | **0** |
| Local RPC errors | 0 |
| Lowest / highest match | 0 / 11,492,708 |

The ladder is stride 1000 over the full range, plus genesis and the archive tip.
Root at h=11,492,708 is `0xd2e265ac1a6ef96071d54e6f471e97a43aff21a8b600d0ef4f75fb70e80a14de`.

Reproduce with:

```
scripts/compare-local-roots-against-reference.py \
  --local http://127.0.0.1:42333 \
  --reference-file data/reference/reference_stateroots_stride1000.jsonl
```

### Checks against the obvious ways this could be wrong

- **Not just round heights.** Eight arbitrary non-stride heights — 1, 7,
  2,718,281, 3,141,593, 5,555,555, 7,777,777, 9,999,999, 11,492,707 — were
  compared individually against the seeds. All eight matched.
- **Not proxied from a peer.** The node answering those queries ran with
  `max_connections = 0` and reported `getconnectioncount` of 0. It had no peer
  to relay a root from, and it still answered correctly at arbitrary heights.
- **The trie is really there.** `findstates` walks the local MPT and returns
  real NeoToken storage entries; `getproof` on one of those keys yields a
  1,956-byte Merkle proof; `verifyproof` validates that proof against the root
  and returns the stored value. The root is the root of a materialized local
  trie, not a recorded number.
- **The chain content is genuine.** Block hashes match the seeds at h=0,
  1,000,000, 6,000,000, and 11,492,708.
- **The reference is not a single source.** 14 randomly sampled ladder heights
  from 614,000 to 10,662,000 were re-fetched from `mainnet1.neo.coz.io`, a
  different operator, and agreed on all 14.

## Live P2P segment

The archive ends at 11,492,708; MainNet's tip is beyond it. Continuing that
database over P2P with StateRoot enabled exercises the live sync path, which
archive import never touches. That segment is compared **per block**, not
sampled, by `scripts/continuous-stateroot-validation.py`; status in
`reports/stateroot-parity/live-segment-status.json`.

The local MPT keeps pace with P2P persistence in this mode — `local_state_height`
tracks the block count. Roots can briefly lag by a few blocks while the bounded
asynchronous StateService pipeline flushes, which surfaces as a transient
`-106 Unknown state root`; the validator pauses and retries rather than failing.

## What remains open

- Per-block comparison across the full 11.49M archive range. That needs ~23M RPC
  calls against public infrastructure; the strided ladder is the deliberate
  substitute, and its limits are stated in `reports/stateroot-parity/README.md`.
- Later-hardfork boundary evidence as new hardforks activate.

## Scope limits of replay as a method

Archive replay never proposes a block and never serves an external client, so it
cannot exercise dBFT consensus or third-party RPC compatibility. Both gaps were
closed separately on a mixed neo-rs / neo-cli / neo-go private network, which
surfaced four defects invisible to replay, one of them consensus-critical (a
block assembled against a speculatively-advanced parent hash, forking the node
off the network after ~1800 blocks). See the v0.12.0 CHANGELOG entry.
