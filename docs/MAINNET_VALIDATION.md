# MainNet Replay Evidence

What MainNet replay has and has not established, as of v0.12.0.

This document previously reported a single "PASSED — 100% Correct" full-archive
StateRoot validation. That framing did not survive a check against its own
source data and has been replaced. The corrected picture is two separate runs
with two different configurations, described below.

## Summary

Full-history StateRoot parity with the C# reference node is **not established**.
Two distinct bodies of evidence exist, and neither one alone supports that claim:

1. **StateRoot-enabled replay with seed comparison**, covering sampled heights up
   to roughly 3.37M. Roots were fetched from `seed1.neo.org` / `seed2.neo.org` and
   matched. This is genuine parity evidence, bounded to the heights sampled.
2. **StateRoot-disabled ledger continuation** to the archive tip at 11,492,708.
   This measures ingest and execution throughput. It computes no state roots at
   all, so it is not parity evidence at any height.

## Run 1 — StateRoot-enabled, seed-compared

StateRoot-enabled replay campaigns compared computed roots against the public
MainNet seeds at sampled heights. Confirmed matches appear in the performance
reports at, among others, h=1,330,000, 1,400,000, 1,500,000, 1,600,000,
1,700,000, 1,847,000, and a cluster in the 3.26M–3.37M range
(`reports/performance/mainnet-continuation-*.md`,
`mainnet-shadow-observed-3267022-3277022.md`,
`ACCEPTED-mainnet-authoritative-random-index-payload-3357022-3372022-20260719.md`).

The highest `getstateroot` seed comparison recorded in these reports is
**h=3,372,022**.

Separately, `reports/performance/optimistic-signature-verification-20260721.md`
records every arm of an A/B reaching height **3,457,022** with the same
StateRoot. That is agreement between local variants of this node, not a
comparison against C#, and should not be read as seed parity.

Above these heights there is no StateRoot comparison against a reference in this
repository.

## Run 2 — StateRoot-disabled, archive tip

Source: `reports/performance/mainnet-full-archive-no-stateroot-3875677-11492708-20260724.md`

| Metric | Value |
|---|---:|
| Range imported | `3,875,678..11,492,708` |
| Blocks imported this run | 7,617,031 |
| Pre-existing canonical blocks in DB | through h=3,875,677 |
| StateRoot | **disabled** (`--stateroot false`) |
| MPT apply attempts / failures | **0 / 0** |
| Import elapsed | 3,929.03 s |
| End-to-end throughput | 1,938.65 blocks/s |
| Transactions executed | 4,609,575 across 1,543,571 blocks |
| Empty blocks | 6,073,460 |
| Final database footprint | 115 GiB (includes pre-existing MPT namespace) |
| Binary | `neo-node 0.10.0`, revision `dfd9a36d` |
| Node binary SHA-256 | `4b08a08a60e23f48b1a6a94ef02478ed774c024342d0de4109b8046ff87d7163` |

Read the `0 / 0` row carefully. Zero MPT failures here is a consequence of zero
MPT *attempts* — `--stateroot false` suppressed all MPT work. It is not evidence
that roots were computed correctly, and it is not evidence that they were
computed at all.

Note also that this run did not replay 11,492,708 blocks. It continued an
existing database from h=3,875,677 and imported 7.6M blocks on top.

## What remains open

- A StateRoot-enabled replay to the archive tip. The source report states this
  directly: "a full StateRoot-enabled replay to the same height is still
  required." StateRoot finalization and durable MPT publication are named there
  as the dominant unresolved release bottleneck.
- Seed or reference comparison above h≈3.37M.
- Later-hardfork boundary evidence.

## Scope limits of replay as a method

Archive replay never proposes a block and never serves an external client, so it
cannot exercise dBFT consensus or third-party RPC compatibility. Both gaps were
closed separately in v0.12.0 on a mixed neo-rs / neo-cli / neo-go private
network, which surfaced four defects invisible to replay, one of them
consensus-critical (a block assembled against a speculatively-advanced parent
hash, forking the node off the network after ~1800 blocks). See the v0.12.0
CHANGELOG entry.
