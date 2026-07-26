# MainNet StateRoot parity evidence

State-root parity results for neo-rs against the C# Neo public MainNet seeds.
Each file here is machine-generated output from a comparison run; this README
records what was compared, how, and what the result licenses you to claim.

## Why state roots are the parity signal

A Neo N3 state root is the root hash of the MPT over the entire state at a
height: every contract's storage, every balance, every native contract record.
If neo-rs's root at height H equals C#'s, the two implementations hold
byte-identical state at H. That is a far stronger statement than matching block
hashes, which only says the two agree on the block's *contents*, not on the
result of executing it.

Two counters are what distinguish a real StateRoot run from a no-op:
`state_service_mpt_apply_attempts` must track `imported` 1:1, and
`state_service_mpt_apply_failures` must stay 0. `--enable-stateroot` (or
`--stateroot true`) is required on the command line; `[state_service].enabled`
in the config is not sufficient, and a run without it computes no roots at all
while still reporting `0 / 0` for those counters. A previous validation write-up
was wrong for exactly that reason.

## Reference ladders

`data/reference/reference_stateroots_stride1000.jsonl` and
`…stride100.jsonl` hold `{"height", "root"}` records fetched from
seed1–seed5.neo.org with `scripts/fetch-reference-stateroots-strided.py`.

The stride-1000 ladder was independently corroborated: 14 randomly sampled
heights spanning 614,000 → 10,662,000 were re-fetched from
`mainnet1.neo.coz.io`, a different operator, and agreed on all 14. coz rejects
concurrent probes with HTTP 403; space serial requests ~1.5 s apart.

## What a strided ladder does and does not prove

Matching at every sampled height proves state equality at each of those
checkpoints. A divergence introduced between two checkpoints is caught unless it
is fully repaired before the next one — the diverging state would have to be
overwritten back to the C# value within the stride. Persistent divergences (a
wrong stored value, a wrong balance, a missing change-set entry — the shape of
every divergence found in this project so far) are therefore caught.

Per-block comparison across the whole 11.49M chain would need ~23M RPC calls
against public infrastructure and is not a reasonable thing to run. Per-block
comparison of a bounded live segment is, and is what
`scripts/continuous-stateroot-validation.py` does.

## Comparing a local node

```
scripts/compare-local-roots-against-reference.py \
  --local http://127.0.0.1:42333 \
  --reference-file data/reference/reference_stateroots_stride1000.jsonl \
  --status-file reports/stateroot-parity/<name>.json
```
Exit 0 = every compared height matched, 1 = mismatch (the report's
`first_mismatch.height` is the finding), 2 = the local node could not answer.

The local node's per-IP RPC limiter answers `-32001 Too many requests` under a
validator's load, which the validator reports as a local error and pauses on.
Raise `max_requests_per_second` / `rate_limit_burst` under `[rpc]` on the
node being validated.
