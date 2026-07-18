## Why

Full Neo N3 replay remains the strongest bootstrap path, but it requires every
new node to download and execute the entire chain while the current FullState
MPT is much larger than the live canonical state. NeoFS can distribute
authenticated block archives and a compact, validator-certified current-state
checkpoint so full nodes can choose replay or fast bootstrap and light clients
can fetch only locally verifiable point proofs.

## What Changes

- Add a deterministic V1 checkpoint core that binds the Neo network, height,
  canonical block hash, complete signed N3 StateRoot, non-Ledger flat-state
  root, Ledger-state root, block-archive root, and exact protocol/format IDs.
- Add a supplementary domain-separated checkpoint certificate signed by the
  independently trusted StateValidator quorum. This authenticates the Ledger
  and block binding that the existing StateService MPT deliberately excludes;
  it does not change the Neo N3 StateRoot or consensus wire protocol.
- Export exact current non-Ledger and Ledger key/value streams from one frozen
  coordinated snapshot, plus immutable raw block segments, into deterministic,
  bounded, content-addressed chunks.
- Publish and retrieve those chunks through a minimal NeoFS transport using a
  high-replication metadata container and a replicated bulk-data container,
  with resumable downloads and an HTTP/P2P fallback.
- Add a pre-startup importer that verifies all content and trust anchors,
  rebuilds the canonical Neo MPT from flat state, validates Ledger and block
  tips, and atomically activates a complete database generation.
- Add a proof gateway and light verifier for current-state membership and
  non-membership, transaction inclusion, and certified Ledger-row point
  proofs. Every proof is verified locally against the accepted checkpoint.
- Preserve full replay from NeoFS or P2P as the trust-minimized path and keep
  checkpoint bootstrap explicit and disabled by default until MainNet parity,
  crash, corruption, and continuation gates pass.
- Exclude ZK proofs, historical-MPT distribution, compact-MPT transport,
  erasure coding, direct NeoFS trie traversal, background/lazy MPT activation,
  application-log archives, notification archives, and arbitrary range proofs
  from V1.

## Capabilities

### New Capabilities

- `neofs-verifiable-checkpoints`: Deterministic checkpoint and certificate
  formats, NeoFS distribution, full-node replay/checkpoint bootstrap, atomic
  import, and light-client point-proof verification.

### Modified Capabilities

None.

## Impact

This change affects checkpoint format and verification code, coordinated MDBX
snapshot/export APIs, StateService StateRoot and MPT rebuild integration,
Ledger record export/import, static block archives, node startup and recovery,
NeoFS/HTTP transport, configuration, RPC proof serving, metrics, and MainNet
replay tooling. It adds no Neo consensus message, VM semantic, MPT encoding,
StateRoot, block, transaction, or canonical Ledger schema change.
