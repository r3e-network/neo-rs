# neo-rs Architecture Inventory (factual baseline)

> Ground-truth map captured 2026-05-29 on `refactor/restore-green-baseline` (HEAD 0e95b69d).
> Baseline is GREEN: `cargo check --workspace --all-targets` passes.
> C# reference baseline: Neo v3.9.1/2 in `neo_csharp/` (+ VM v3.9.0 in `neo_csharp_vm/`).
> This is the factual substrate for the audit; judgments live in `audit-findings.md`.

## Crate map (Rust LOC vs C# counterpart)

| Crate | LOC | Files | C# counterpart | Notes |
|---|--:|--:|---|---|
| neo-primitives | 7,959 | 44 | (parts of Neo core) | Foundation; carries many shared enums + UInt160/256. Kitchen-sink risk. |
| neo-config | 1,511 | 6 | ProtocolSettings | Overlaps neo-core/protocol_settings.rs (19.5K). |
| neo-crypto | 6,328 | 20 | Neo/Cryptography | Has mpt_trie (overlap w/ state_service). |
| neo-storage | 5,145 | 33 | Neo/Persistence + LevelDB/RocksDB | Overlaps neo-core/persistence. |
| neo-io | 2,485 | 22 | Neo.IO | Has compression (overlap w/ neo-core/compression facade). |
| neo-json | 1,325 | 8 | Neo.Json | Bespoke ordered JSON. |
| **neo-core** | **102,451** | **520** | Neo.csproj + inlined plugins | 54% of repo. Decomposition target. |
| neo-p2p | 2,140 | 23 | (part of Neo/Network) | INVERTED: bulk of P2P is in neo-core/network (17K). Many facade re-exports. |
| neo-rpc | 36,993 | 125 | Plugins/RpcServer (4.1K)+RestServer (5.8K) | ~3.7x C#. Bloat target. |
| neo-consensus | 7,528 | 37 | Plugins/DBFTPlugin (2.2K) | |
| neo-tee | 5,033 | 16 | (none — Rust-only) | Optional node feature. |
| neo-hsm | 1,732 | 15 | Plugins/SignClient-ish | Optional node feature. |
| neo-telemetry | 1,988 | 10 | (none) | Overlaps neo-core/telemetry (0.8K). |
| neo-node | 8,313 | 26 | Neo.CLI + ConsoleService | Binary. |
| tests | 7,792 | — | — | Integration tests. |
| benches-package | 281 | — | — | |

## Inter-crate dependency edges

```
neo-config      -> (none)
neo-json        -> (none)
neo-primitives  -> (none)
neo-io          -> neo-primitives
neo-crypto      -> neo-io neo-primitives
neo-storage     -> neo-primitives
neo-hsm         -> neo-crypto
neo-tee         -> neo-crypto
neo-telemetry   -> (none)
neo-p2p         -> neo-crypto neo-io neo-primitives
neo-core        -> neo-config neo-crypto neo-json neo-p2p neo-primitives neo-storage neo-vm-rs
neo-consensus   -> neo-core neo-crypto neo-io neo-primitives neo-vm-rs
neo-rpc         -> neo-config neo-core neo-crypto neo-io neo-json neo-primitives neo-vm-rs
neo-node        -> neo-consensus neo-core neo-crypto neo-hsm neo-p2p neo-rpc neo-tee neo-telemetry neo-vm-rs
```
External VM: `neo-vm-rs = { path = "../neo-vm-rs" }` (pure VM semantics). VM host folded into neo-core/src/neo_vm.

## neo-core module breakdown (LOC)

| Module | LOC | Disposition hypothesis (to verify in audit) |
|---|--:|---|
| smart_contract/ | 35,313 | Core — native contracts + ApplicationEngine. Stays, but native could be own crate. |
| network/ | 16,964 | P2P + payloads. Payloads (Block/Tx/Signer) ledger-coupled → stay. P2P transport → candidate to consolidate with neo-p2p. |
| neo_vm/ | 12,496 | VM host. Stays (per north-star). |
| ledger/ | 6,023 | Core ledger. Stays. |
| oracle_service/ | 5,294 | Inlined C# OracleService plugin. Candidate own crate. |
| state_service/ | 4,386 | Inlined C# StateService plugin. Candidate own crate. MPT overlap w/ neo-crypto. |
| neo_system/ | 3,918 | Core orchestration. Stays. |
| wallets/ | 3,486 | Inlined Neo/Wallets. Candidate own crate (neo-wallets). |
| persistence/ | 2,908 | OVERLAP w/ neo-storage. Merge target. |
| tokens_tracker/ | 2,068 | Inlined C# TokensTracker plugin. Candidate own crate. |
| actors/ | 1,814 | Hand-rolled Akka-style actor system. Ecosystem-replacement candidate (tokio). |
| telemetry/ | 796 | Dep-free metrics; vs neo-telemetry crate. Verify intentional. |
| witness_rule/ + witness_rule.rs | ~1,500 | Core. Stays. |
| builders/ | 621 | Fluent builders. Stays. |
| application_logs/ | 562 | Inlined C# ApplicationLogs plugin. Candidate own crate. |
| services/, extensions/, events/, properties/ | small | Verify each. |
| compression/ | 23 | FACADE → neo-io. Eliminate. |

## Confirmed facade / duplication candidates (from export surface)

- **neo-p2p** re-export facades: `oracle_response_code`, `transaction_removal_reason`, `witness_condition_type`, `witness_rule_action` → all `pub use neo_primitives::...`.
- **contains_transaction_type** duplicated: neo-primitives AND neo-p2p.
- **neo-core/compression/mod.rs** (23 LOC) → neo-io::compression.
- **neo-storage::persistence::data_cache + key_builder** vs **neo-core/persistence/data_cache + smart_contract/key_builder**.
- **neo-crypto::mpt_trie** vs **neo-core/state_service/state_store** (MPT/state root).
- **neo-config::ProtocolSettings** vs **neo-core/protocol_settings.rs** (19.5K) — protocol config split across two places.
- **neo-primitives** carries non-primitive concerns: `rpc_exception`, `storage`, `blockchain`, `verification`, `serializable_payload`.

## C# reference map

- Core: `neo_csharp/src/Neo/{SmartContract(13K),Network(7.4K),Cryptography(2.5K),Wallets(2.5K),Ledger(1.9K),Extensions,Persistence,Builders,IO,Sign}`
- Native contracts: `neo_csharp/src/Neo/SmartContract/Native/` — incl. **Treasury**, **WhitelistedContract**, **Notary** (v3.9 additions).
- Plugins: `neo_csharp/src/Plugins/{RpcServer,RestServer,DBFTPlugin,ApplicationLogs,TokensTracker,StateService,OracleService,LevelDBStore,RocksDBStore,SignClient,SQLiteWallet,StorageDumper}`
- VM: `neo_csharp_vm/src/Neo.VM/{,Collections,JumpTable,Types,StronglyConnectedComponents}`
