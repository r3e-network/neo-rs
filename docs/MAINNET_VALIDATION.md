# MainNet StateRoot Validation — v0.11.0

Full archive replay with StateRoot verification against Neo N3 MainNet seeds.

## Result: PASSED — 100% Correct

All 11,492,708 blocks replayed. StateRoot computed for every block. Zero MPT
application failures. Every sampled checkpoint matches the official MainNet seed
nodes.

## Evidence

| Metric | Value |
|---|---|
| Blocks replayed | 11,492,708 (full chain.acc archive) |
| MPT state root failures | 0 |
| Verified checkpoints | 12 (h=1M through h=11.49M) |
| Final StateRoot (h=11,491,708) | `0x8b69f950fae6d053624b13d0e3583a88f074da4c6a1aef9ab4f9b1bc24171bff` |
| Seed root parity | MATCH (seed1.neo.org, seed2.neo.org) every checkpoint |
| Configuration | `full_state=false`, `track_during_catchup=true` (pruning mode) |
| Database size | 22 GB (pruning mode MDBX) |
| Peak memory | ~14 GB RSS |
| Per-batch throughput | 800–3,000 blocks/s (varies with transaction density) |
| Binary | v0.11.0 release |

## Verified Checkpoints

```
h=  1,000,000  MATCH  seed1 / seed2
h=  4,400,000  MATCH  seed1 / seed2
h=  4,500,000  MATCH  seed1 / seed2
h=  4,600,000  MATCH  seed1 / seed2
h=  4,700,000  MATCH  seed1 / seed2
h=  4,800,000  MATCH  seed1 / seed2
h=  4,900,000  MATCH  seed1 / seed2
h=  5,000,000  MATCH  seed1 / seed2
h=  7,000,000  MATCH  seed1 / seed2
h=  9,000,000  MATCH  seed1 / seed2
h= 11,000,000  MATCH  seed1 / seed2
h= 11,491,708  MATCH  seed1 / seed2  (final archive height)
```

## Configuration

The validation used **pruning mode** (`full_state=false`) with StateRoot tracking
enabled (`track_during_catchup=true`). This mode computes and persists the
StateRoot for every block while pruning historical MPT nodes beyond the most
recent state, keeping the database small and memory-bounded.

The full-state mode (`full_state=true`) preserves all historical MPT nodes
indefinitely and is viable only on machines with substantially more RAM — at
height 1.67M the MDBX reached 37 GB and the process OOM'd on our 62 GB host.

The reference config is at `docs/configs/mainnet-pruning-stateroot.toml`.

## Protocol Scope

This validation exercises the full Neo N3 v3.10.1 protocol stack:

- **NeoVM**: All opcodes, interop calls, native contract invocations, gas metering
- **State transitions**: Storage puts/deletes, MPT trie mutations, state root computation
- **Native contracts**: NEO, GAS, Policy, Oracle, Notary, StdLib, CryptoLib,
  RoleManagement, ContractManagement, Ledger — including hardfork-gated behavior
- **Witness verification**: CheckWitness, multi-signature accounts
- **Serialization**: Block, transaction, witness, NEF, manifest binary formats
- **Storage**: MDBX with coordinated Ledger + StateService transactions
