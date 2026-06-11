# neo-rs vs C# Neo v3.9.1/2 — Full Audit Findings

_Generated 2026-05-29 from 14-agent parallel audit (3.2M subagent tokens). Raw JSON: audit-raw.json_

## Totals
- Findings: 108  (critical 12, high 23, medium 26, low 47)
- Consensus-breaking: 24
- Interop-breaking: 15

## Verdicts by dimension
- **Native contracts parity** — major-gaps (8)
- **NeoVM semantics parity** — major-gaps (11)
- **P2P wire protocol parity** — minor-gaps (6)
- **dBFT consensus parity** — minor-gaps (7)
- **Cryptography parity (neo-crypto vs neo_csharp/src/Neo/Cryptography + CryptoLib native)** — minor-gaps (7)
- **Binary serialization / IO parity** — minor-gaps (6)
- **Ledger / blockchain semantics parity (Block, Transaction, Header, TransactionAttribute, MemoryPool, verification pipeline, genesis)** — major-gaps (10)
- **RPC/REST API surface parity** — minor-gaps (9)
- **neo-core decomposition & crate boundaries** — not-applicable (8)
- **neo-rpc architecture & bloat** — minor-gaps (8)
- **storage vs persistence layering** — minor-gaps (6)
- **Foundation crates cohesion + idiomatic Rust + ecosystem reuse** — minor-gaps (6)
- **Ecosystem / best-in-class crate adoption (reth + polkadot-sdk patterns)** — not-applicable (7)
- **Holistic crate-cohesion matrix** — minor-gaps (9)

---

## Binary


## serialization


## /


## IO


## parity


## Cryptography


## parity


## (neo-crypto


## vs


## neo_csharp/src/Neo/Cryptography


## +


## CryptoLib


## native)


## dBFT


## consensus


## parity


## Ecosystem


## /


## best-in-class


## crate


## adoption


## (reth


## +


## polkadot-sdk


## patterns)


## Foundation


## crates


## cohesion


## +


## idiomatic


## Rust


## +


## ecosystem


## reuse


## Holistic


## crate-cohesion


## matrix


## Ledger


## /


## blockchain


## semantics


## parity


## (Block,


## Transaction,


## Header,


## TransactionAttribute,


## MemoryPool,


## verification


## pipeline,


## genesis)


## Native


## contracts


## parity


## neo-core


## decomposition


## &


## crate


## boundaries


## neo-rpc


## architecture


## &


## bloat


## NeoVM


## semantics


## parity


## P2P


## wire


## protocol


## parity


## RPC/REST


## API


## surface


## parity


## storage


## vs


## persistence


## layering


