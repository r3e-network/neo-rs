# MainNet Full Archive Continuation Without StateRoot

Date: 2026-07-24. Status: completed supplemental performance campaign.

This run measures canonical Ledger sync and transaction/native execution with
StateRoot explicitly disabled. It is not evidence for the required 2,000 BPS
StateRoot-enabled target and does not prove full-history StateRoot parity.

## Command

```bash
target/release/neo-node \
  --config data/neo-v3101-staged-replay/neo_mainnet_validate_deferred.toml \
  --storage-path data/neo-v3101-staged-replay/full-mainnet-stateroot-h11492708 \
  --stateroot false \
  --stop-at-height 11492708 \
  --import-chain data/neo-v3101-staged-replay/fast-sync-cache/chain.0.acc/chain.0.acc
```

The database already contained canonical blocks through height 3,875,677 and
historical MPT data from an earlier StateRoot-enabled replay. This continuation
did not delete that namespace; `--stateroot false` prevented all new MPT apply
work. The trusted local archive path used the configured deferred-validation
import mode rather than P2P download time.

## Environment

- Release profile, `neo-node 0.10.0`, source revision
  `dfd9a36db4a0638c128348a1801bc9326b037706` (the release is versioned after
  this completed campaign).
- Node binary SHA-256:
  `4b08a08a60e23f48b1a6a94ef02478ed774c024342d0de4109b8046ff87d7163`.
- 8 visible vCPUs, Intel Core Ultra 9 285K host, 62 GiB RAM, VMware guest.
- Archive: 9.4 GiB `chain.0.acc`.
- Final database footprint: 115 GiB, including the pre-existing MPT namespace.

## Result

| Metric | Result |
|---|---:|
| Absolute range | `3,875,678..11,492,708` |
| Imported blocks | 7,617,031 |
| Import elapsed | 3,929.030779496 s |
| End-to-end throughput | **1,938.6539 blocks/s** |
| Transaction-bearing blocks | 1,543,571 |
| Transactions | 4,609,575 |
| Transaction-stage elapsed | 2,287.374292474 s |
| Transaction-stage throughput | 674.8222 blocks/s |
| Empty blocks | 6,073,460 |
| Empty-stage elapsed | 121.152835048 s |
| Empty-stage throughput | 50,130.5479 blocks/s |
| Final batch | 7,031 blocks at 4,869.4895 blocks/s |
| MPT apply attempts / failures | 0 / 0 |
| Final MDBX commit window | 937,867 us for 15,422 entries |

The two stage rates use different denominators and exclude shared finalization
cost, so they do not mathematically combine into overall BPS. Overall BPS is
`7,617,031 / 3,929.030779496` and includes canonical import finalization and
Ledger persistence for the measured interval.

## Decision

The run demonstrates that the StateRoot-disabled archive path can sustain about
1.94k end-to-end BPS over 7.6 million consecutive MainNet blocks on this host.
It establishes no paired optimization delta. StateRoot finalization and durable
MPT publication remain the dominant unresolved release bottleneck, and a full
StateRoot-enabled replay to the same height is still required.
