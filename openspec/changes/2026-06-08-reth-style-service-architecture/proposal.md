# Reth-style service architecture for Neo

## Why

At change creation, `neo-actors` was an Akka-style actor framework directly
ported from the C# Neo implementation. That architecture has since been
removed. Rust's idiomatic concurrency model is `async` + Tokio tasks, typed
channels, and explicit service ownership. Reth and polkadot-sdk both provide
useful service-composition references.

The current work continues that migration by adopting Reth's ownership,
provider, immutable ChainSpec, static composition, RPC-boundary, pool, and
storage patterns where they fit Neo, without importing Ethereum semantics or a
second VM.

## Reth architecture adapted for Neo

| Reth pattern | Neo adaptation | Boundary |
|---|---|---|
| Immutable `ChainSpec` | `neo_config::NeoChainSpec` | One validated network identity, genesis definition, protocol rule set, and hardfork schedule |
| Provider and provider-factory capabilities | Ledger and StateService providers with associated concrete provider types | Freeze a view and expose narrow reads without leaking the backend |
| Validator, pool core, and pool handle separation | Typed `neo-mempool` admission, private indexes, and explicit relay at the node boundary | Keep validation and mutation policy in one owner while networking remains separate |
| Static node composition and required builders | `NodeCoreBuilder` and `NodeBuilder` | Make incomplete ownership graphs unrepresentable |
| RPC API/types/server/client separation | Feature-independent RPC types and protocol codecs plus independent client/server features | Avoid client-owned shared models and server-to-client coupling |
| Dedicated trie and persistence domains | `neo-trie`, `neo-state-service`, `neo-storage`, static ledger files, and state packs | Keep hashing, trie mechanics, state policy, database mechanics, and immutable history distinct |
| Network manager plus cheap handle | `neo-network` services and `NetworkHandle` | Move peer/session truth into the manager and expose bounded commands plus owner-derived snapshots; broadcast events are notifications, not a second state store |

Neo does not adopt Ethereum Engine API, forkchoice, account-state execution,
devp2p, Ethereum transaction subpools, or an EVM-style type universe. Neo dBFT,
NeoVM, native contracts, StateService, Neo wire payloads, and Neo C# v3.10.1
behavior remain authoritative.

Hot paths prefer concrete types, associated types, and generics. Dynamic trait
objects are reserved for genuinely open runtime extension boundaries; service
APIs do not use `async_trait` merely to resemble another node.

## What gets replaced

| Old (actor) | New (service) |
|---|---|
| ActorSystem | Node builder |
| ActorRef | mpsc::Sender<T> (cheap to clone) |
| actor_cast! | service.method(...).await |
| Mailbox dispatch | tokio::select! over channels |
| Ask pattern | request/response via oneshot channel |

## Migration status

The actor-to-service migration is complete: runtime capabilities, blockchain
and network services, node composition, consumer migration, and actor removal
have one current implementation. This change now focuses on eliminating the
remaining duplicate ownership paths and tightening static composition.

## Verification

- `cargo test --workspace --no-run --locked`
- strict Clippy for every touched crate
- focused protocol and parity tests
- architecture ownership guards
- strict OpenSpec validation
- no `neo-actors`, actor macros, `ActorRef`, or actor compatibility facade
