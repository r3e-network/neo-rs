## 1. Canonical Formats And Trust

- [x] 1.1 Create a dependency-light `neo-checkpoint` crate for V1 core, certificate, stream, chunk, transport-manifest, trust-anchor, and proof types with explicit format versions and typed errors
- [x] 1.2 Implement bounded canonical encode/decode and domain-separated checkpoint ID/sign-data calculation for `CheckpointCoreV1` and `CheckpointCertificateV1`
- [x] 1.3 Implement deterministic state/Ledger row encoding, RFC6962-style stream commitments, chunk descriptors, fixed compression identity, and strict geometry validation
- [ ] 1.4 Implement locally anchored StateValidator-set resolution, canonical N3 StateRoot witness verification, supplementary BFT checkpoint-certificate verification, and signer-set mismatch rejection
- [ ] 1.5 Add frozen binary test vectors and mutation tests for every committed field, noncanonical length, overflow, trailing byte, duplicate signature, wrong network, wrong protocol digest, wrong root, and circular self-signed validator set

## 2. Frozen Export And Certification

- [ ] 2.1 Add a coordinated read-only MDBX snapshot API that spans canonical and StateService namespaces without opening independent transactions
- [ ] 2.2 Implement a bounded raw-row exporter that partitions every live row exactly once into non-Ledger and Ledger streams in strict raw-key order
- [ ] 2.3 Build deterministic contiguous header/block segment catalogs through H and validate genesis, parent linkage, consensus witnesses, transaction Merkle roots, and archive horizon
- [ ] 2.4 Implement offline chunk/core export with before/after height-root-tip checks, deterministic zstd settings, atomic local publication, and exact row/value/chunk accounting
- [ ] 2.5 Implement an independent offline verifier that recomputes stream/archive commitments and rebuilds the pruning-mode Neo MPT to the signed StateRoot
- [ ] 2.6 Implement StateValidator signer and quorum-collector tooling that verifies local canonical data before signing and rejects duplicate, invalid, foreign, and below-quorum contributions
- [ ] 2.7 Re-export one frozen fixture with independent processes and prove byte-identical core, stream roots, archive roots, checkpoint ID, and verifier result

## 3. Minimal NeoFS Distribution

- [ ] 3.1 Add a bounded checkpoint object-source interface and a NeoFS REST implementation using direct object IDs, streamed reads, timeouts, cancellation, and no Oracle text/data caps
- [ ] 3.2 Implement publisher configuration for one REP5 metadata container and one REP3 bulk container, chunk upload/read-back verification, and descriptor-last publication
- [ ] 3.3 Implement resumable content-addressed `.part` downloads with encoded/logical hash checks and bounded NeoFS, HTTP, and P2P locator fallback
- [ ] 3.4 Add network-info price snapshots and a non-authoritative GAS capacity estimate to publish reports without hard-coding NeoFS rates into checkpoint validity
- [ ] 3.5 Test corrupt objects, unavailable objects, stale locators, truncated streams, retry exhaustion, cancellation, timeouts, duplicate chunks, and descriptor publication before complete upload

## 4. Staged Full-Node Import

- [ ] 4.1 Add versioned generation layout, exclusive importer lock, staging/final/READY states, durable acceptance-floor metadata, and atomic fsynced `CURRENT` pointer operations
- [ ] 4.2 Move certified-checkpoint orchestration before live MDBX/archive opening and reject unsupported split canonical/StateService generation layouts
- [ ] 4.3 Implement trust-first and content-complete preflight with checked aggregate arithmetic, disk reserve, row/value/chunk/decompression/memory/file/concurrency limits, and no general archive extraction
- [ ] 4.4 Stream exact state and Ledger rows into staging canonical storage, install verified header/block segments, and preserve canonical raw bytes and height indexes
- [ ] 4.5 Implement a bounded canonical pruning-mode MPT rebuild from the state stream, persist complete signed StateRoot/certificate metadata, and compare the exact root at H
- [ ] 4.6 Verify stream roots/counts, Ledger tip, block tip/hash, archive horizon, MPT root, deterministic read digest, full reopen state, and checkpoint identity before writing READY
- [ ] 4.7 Implement directory/file durability and atomic activation so every injected crash or `ENOSPC` boundary exposes exactly the previous complete generation or the new complete generation
- [ ] 4.8 Implement offline rollback, quarantine, incomplete-staging cleanup, retained-generation policy, anti-rollback rejection, and same-height quorum-equivocation handling
- [ ] 4.9 Add explicit `replay` and disabled-by-default `certified-checkpoint` configuration, RPC/metrics identity, startup validation, and no silent verification downgrade

## 5. Replay And MainNet Continuation

- [ ] 5.1 Route NeoFS/HTTP block segments through the ordinary canonical replay/import pipeline without bypassing header, witness, transaction, VM, Ledger, or StateRoot validation
- [ ] 5.2 Test locator fallback and first-invalid-height diagnostics against missing, reordered, forked, malformed, and internally hashed but protocol-invalid block segments
- [ ] 5.3 Import a certified staged MainNet checkpoint on a clean clone and compare complete canonical rows, Ledger rows, MPT root, signed StateRoot, headers, blocks, archive horizon, and reopen digest with the source node
- [ ] 5.4 Execute H+1 strict MainNet continuation windows across transaction-heavy and hardfork boundaries and compare block hashes, VM artifacts, Ledger values, MPT roots, signed StateRoots, restart, and first-divergence evidence
- [ ] 5.5 Run full replay from the same NeoFS block archive and prove it reaches the same checkpoint identity inputs and post-H continuation state as peer/archive replay

## 6. Light Point-Proof Access

- [ ] 6.1 Implement an untrusted proof-gateway endpoint for current non-Ledger value/absence with canonical Neo MPT proofs and explicit history-floor behavior
- [ ] 6.2 Implement a persisted/rebuildable sorted-Ledger Merkle proof index with row inclusion and adjacent-boundary or edge non-membership proofs against the certified Ledger root
- [ ] 6.3 Implement block/header/transaction point retrieval and transaction Merkle inclusion proofs against the accepted canonical header chain
- [ ] 6.4 Implement a dependency-light client verifier for trust anchor, signed StateRoot, checkpoint certificate, anti-rollback state, MPT proofs, Ledger proofs, and transaction inclusion
- [ ] 6.5 Test malicious gateways that alter values, omit boundaries, mix checkpoints, replay stale roots, forge absence, return wrong blocks, or request unsupported range, prefix, historical, log, and notification behavior

## 7. Operations And Promotion Gates

- [ ] 7.1 Add bounded-label metrics and machine-readable reports for source retries, download, hash, decompression, certificate verification, import, MPT rebuild, archive validation, fsync, reopen, proof service, rollback, and continuation
- [ ] 7.2 Run corruption and hostile-geometry matrices covering every core/chunk/row/archive field, decompression bomb, arithmetic overflow, disk exhaustion, file-descriptor/memory limits, and cancellation boundary
- [ ] 7.3 Run deterministic crash injection before and after every object publication, chunk write, database batch, archive install, MPT publication, fsync, READY, generation rename, CURRENT replacement, cleanup, and rollback step
- [ ] 7.4 Measure export, download, import, MPT rebuild, disk/RSS/CPU/network cost, proof latency, NeoFS price snapshot, and recovery time on named production-equivalent hardware and filesystem profiles
- [ ] 7.5 Perform independent restore drills from NeoFS and fallback sources, verify anti-rollback/equivocation behavior, and retain source/checkpoint/binary digests with exact success or first-failure evidence
- [ ] 7.6 Keep checkpoint authority disabled by default until all deterministic export, certificate, complete parity, MainNet continuation, crash, corruption, rollback, resource, and independent-host gates pass
- [ ] 7.7 Audit V1 scope before promotion and reject accidental dependencies on ZK, historical/compact MPT transport, erasure coding, direct NeoFS trie traversal, lazy activation, checkpoint deltas, range proofs, logs, or notifications
