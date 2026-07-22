# Reth-style service architecture

The historical actor and additive compatibility stages are complete. The
workspace now has one Tokio service architecture; obsolete actor paths are not
retained as a second implementation.

## Completed service migration

- [x] 1.1 Define narrow runtime service capabilities and typed command/event boundaries.
- [x] 1.2 Move blockchain orchestration to the canonical `neo-blockchain` service loop.
- [x] 1.3 Move peer/session orchestration to `neo-network` services and handles.
- [x] 1.4 Compose required node capabilities through `neo-system` builders.
- [x] 1.5 Migrate consumers and delete actor crates, macros, handles, and compatibility paths.

## Current ownership consolidation

- [x] 7.1 Add the Reth-inspired root `AGENTS.md`, canonical crate layers, and architecture guards.
- [x] 7.2 Make `NeoChainSpec` the immutable chain identity used by genesis, composition, P2P, mempool, RPC, and Oracle services.
- [x] 7.3 Move operator transaction-pool capacity to `neo_mempool::TxPoolConfig` and require core node components at builder construction.
- [x] 7.4 Remove dead `NodeTypes`, duplicate configuration/provider traits, VM contract wrappers, execution aliases, and serialization storage facades.
- [x] 7.5 Split RPC client/server features, move shared models/codecs to neutral modules, and delete unused legacy RPC models.
- [x] 7.6 Remove `neo-payloads -> neo-storage`; move service lifecycle capabilities to `neo-runtime` and delete the `VerifyResult` facade.
- [x] 7.7 Move Neo MPT mechanics from `neo-crypto` to the exclusive `neo-trie` crate without changing bytes, hashes, proofs, or algorithms.
- [x] 7.8 Establish one typed transaction-admission boundary, move state-independent validation outside the pool write lock, and delete the unused router/cached-admission scaffolding.
- [x] 7.9 Move the RPC-only exception type out of `neo-primitives` and into the canonical `neo-rpc` server boundary.
- [x] 7.10 Delete the `neo-manifest::CallFlags` re-export facade and migrate every caller to the canonical primitive type.
- [x] 7.11 Make `neo-network` the exclusive owner of `MessageCommand` and its parse error; delete compatibility wrappers for foundation primitives and the duplicate P2P error vocabulary.
- [x] 7.12 Pass formatting, focused parity tests, strict Clippy, workspace compile, dependency hygiene, architecture guards, and strict OpenSpec validation.
