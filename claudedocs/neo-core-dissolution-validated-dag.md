# neo-core Dissolution — Validated Target Crate DAG (reth-grounded)
_Generated 2026-05-31 by the `neo-core-dissolution-plan` workflow (23 agents, 2.4M tokens). Supersedes the layer table in neo-core-decomposition-plan.md; the seam analysis (S1-S9, A1-A6) there still applies._

## reth boundary principles (the model)
- Types vs API-traits vs concrete-impl is split across THREE crates, not one. Example: reth-db-models/reth-primitives-traits (types) → reth-db-api (KV/table trait abstraction) → reth-db (MDBX impl); and reth-storage-api (provider traits) → reth-provider (concrete provider over db+trie). Consumers depend on the -api crate; only the node-assembly layer pulls in the heavy impls.
- The '-api' suffix is a deliberate, consistent convention for trait-seam crates: storage-api, db-api, network-api, node-api, rpc-api, rpc-eth-api, stages-api. Each is a thin interface crate that its concrete sibling (db, network, rpc, stages) implements. This is how reth lets you swap or mock implementations.
- Trait crates sit LOW and never depend on their implementors. reth-storage-api depends on reth-db-api only as an OPTIONAL feature and never on reth-db; reth-consensus has only 2 reth deps (primitives-traits, execution-types); reth-evm depends on storage-api (a trait) not on a concrete provider. The dependency arrow always points down toward abstractions (Dependency Inversion at crate granularity).
- Strict acyclic downward-only dependency rule: L0 → L1 → L2 → L3 → L4 → L5. Lower crates never import upward. Cycles that would otherwise form (e.g. node needs provider, provider needs node-config) are broken by extracting the shared contract into a lower -api/-types crate (reth-node-types, reth-node-api) that both sides depend on.
- Protocol-specific concrete types are isolated from the generic trait seam. reth-primitives-traits (generic, alloy-based traits) is separate from reth-ethereum-primitives (concrete ETH types), and reth-evm (abstract) is separate from reth-ethereum-evm (ETH impl). This keeps the abstraction layer reusable across chains/forks (the same pattern Optimism's reth fork exploits).
- Single responsibility enforced by fine granularity: ~138 workspace crates, each doing one thing. Networking alone is ~14 crates (discv4, discv5, dns, ecies, eth-wire, eth-wire-types, p2p, peers, network, network-api, network-types, nat, banlist, downloaders), splitting wire types from wire logic from the network state machine from the public API.
- 'types' crates are split out beneath their domain so siblings can share data shapes without depending on logic: reth-network-types, reth-stages-types, reth-prune-types, reth-static-file-types, reth-rpc-eth-types, reth-eth-wire-types, reth-db-models, reth-execution-types, reth-trie-common. These are leaf-ish and break what would otherwise be peer cycles.
- Errors are their own low-level crates (reth-errors, reth-storage-errors, reth-execution-errors) so any layer can return/match them without depending on the subsystem that produces them.
- Cross-cutting infra (tasks, metrics, tracing, tokio-util, fs-util, etl, config) lives in standalone protocol-agnostic leaf crates that anyone may depend on, ensuring observability/runtime plumbing never forces an upward dependency into node logic.
- The node-builder is the only 'fat' orchestration crate (45+ deps) and it sits strictly at the top; complexity is concentrated at the assembly layer instead of being smeared across the system, keeping every lower crate lean and independently testable.

## Target crate DAG (acyclic, downward-only)
- **L0 neo-actors** [exists] — Generic tokio actor runtime (Actor/supervision/mailboxes/scheduling). Zero neo coupling. (TaskExecutor+EventStream deferred-split into neo-tasks/neo-events later, not now.)  
  deps: —
- **L0 neo-crypto** [grow] — Crypto primitives: hashing, ECC (secp256r1/k1), ed25519, BLS12-381, signatures, MPT trie, merkle, bloom. Owns the shared Neo signature-redeem-script / script-hash derivation primitive (de-triplicated from core/hsm/tee).  
  deps: neo-primitives, neo-io
- **L0 neo-io** [grow] — Binary (de)serialization seam: Serializable trait, BinaryWriter/MemoryReader, var-int, plus LZ4 compress/decompress (folded in from neo-core::compression). Caching machinery REMOVED.  
  deps: neo-primitives
- **L0 neo-json** [exists] — C#-parity JSON model (JToken/JObject/JArray/OrderedDictionary + JSONPath) and the single source of truth for JavaScriptEncoder.Default escaping.  
  deps: neo-io
- **L0 neo-primitives** [grow] — Pure L0 value types + protocol enums: UInt160/256, BigDecimal, base58, TriggerType, CallFlags, ContractParameterType, Hardfork, WitnessScope, OracleResponseCode. Absorbs constants.rs + time_provider.rs leaf utilities. Dead blockchain/ trait-seams DELETED.  
  deps: —
- **L0 neo-vm-rs** [exists] — Pure stackless VM: opcodes, StackValue, ExecutionEngineLimits, value semantics (no_std-capable). Owns ScriptBuilder + script_validation (pure bytecode emit/validate, moved from neo-core).  
  deps: neo-io
- **L1 neo-config** [merge-target] — Chainspec-style protocol layer ONLY: canonical ProtocolSettings + HardforkManager + NetworkType + GenesisConfig. Dead node-operator settings half (Settings/Node/Rpc/Storage/Logging/Telemetry/Consensus) DELETED.  
  deps: neo-primitives, neo-crypto
- **L1 neo-storage** [grow] — Storage abstraction: Store/ReadOnlyStore/WriteStore/StoreSnapshot/StoreProvider traits + StorageKey/StorageItem/SeekDirection/TrackState/KeyBuilder + DataCache/StoreCache + in-memory backend. RocksDB-specific config moved OUT.  
  deps: neo-io, neo-primitives
- **L1 neo-telemetry** [exists] — Cross-cutting observability leaf: logging init, single Prometheus registry (incl. RPC metric identities), health/readiness HTTP, system-resource monitoring.  
  deps: —
- **L2 neo-storage-rocksdb** [exists] — Concrete RocksDB backend implementing neo-storage trait seam; confines rocksdb dep. Owns RocksDB-specific StorageConfig/Compaction/Compression + read_cache.  
  deps: neo-storage, neo-io
- **L2 neo-vm** [exists] — Stateful VM host over neo-vm-rs: execution loop, ref-counted StackItem+GC, ExecutionContext, InteropHost/Interoperable seam, BinarySerializer/JsonSerializer StackItem codecs, rpc_json.  
  deps: neo-vm-rs, neo-io, neo-primitives, neo-crypto
- **L3 neo-p2p** [grow] — All verifiable chain types + wire codecs: Block, Header, Transaction, Signer, Witness, attributes, ExtensiblePayload, conditions, oracle_response, inventory, WitnessRule + the pure data HeaderCache/TransactionVerificationContext/VerifyResult/LedgerContext; plus existing control payloads + MessageCommand/Flags + ChannelsConfig + timeouts. Verifies via VerificationContext trait — no edge into VM-engine/native. Dead traits.rs + pass-through re-exports DELETED.  
  deps: neo-primitives, neo-io, neo-config, neo-crypto, neo-storage, neo-vm-rs
- **L4 neo-native-contracts** [new] — The 11 native protocol contracts (ContractManagement, NeoToken, GasToken, Policy, Ledger, Oracle, RoleManagement, CryptoLib, StdLib, Notary, Treasury). Injected into engine via NativeRegistry.  
  deps: neo-smart-contract, neo-vm, neo-p2p, neo-storage, neo-config, neo-crypto, neo-native-traits
- **L4 neo-native-traits** [new] — Tiny seam: NativeContract trait + NativeRegistry + EngineHost/InteropContext trait the engine implements (dependency inversion so engine never imports native bodies).  
  deps: neo-vm, neo-primitives
- **L4 neo-smart-contract** [new] — ApplicationEngine execution host + interop syscalls + manifest/ABI/NEF + Contract/ContractParameter model + StorageItemExt. Implements VerificationContext + EngineHost. No native bodies.  
  deps: neo-vm, neo-p2p, neo-storage, neo-config, neo-crypto, neo-io, neo-primitives, neo-native-traits, neo-json
- **L5 neo-tx-builder** [new] — Fluent Transaction/Signer/Witness/WitnessCondition/WitnessRule builders over neo-p2p payload types.  
  deps: neo-p2p, neo-primitives
- **L5 neo-wallets** [new] — KeyPair, NEP-6 wallet, NEP-2/scrypt, BIP32/39, ContractParametersContext/signing-context. Accepts forward edge to neo-smart-contract (Q3 locked).  
  deps: neo-primitives, neo-crypto, neo-p2p, neo-smart-contract, neo-io
- **L6 neo-ledger** [new] — Blockchain actor + MemoryPool + genesis. Duplicate Block collapsed into neo-p2p's; actors hold Arc<dyn SystemContext>.  
  deps: neo-p2p, neo-smart-contract, neo-native-contracts, neo-storage, neo-config, neo-actors, neo-node-traits
- **L6 neo-network** [new] — P2P transport: local_node, remote_node, task_manager, framing, peer lifecycle, relay/inventory caches (folded from neo-io::caching). Holds Arc<dyn SystemContext>.  
  deps: neo-p2p, neo-config, neo-io, neo-actors, neo-node-traits
- **L6 neo-node-traits** [new] — Node trait seams: SystemContext + LedgerService/StateStoreService/MempoolService/PeerManagerService/RpcService + event-handler traits (Committed/Committing/MessageReceived/WalletChanged). Object-safe; return types live in low crates only.  
  deps: neo-p2p, neo-storage, neo-config, neo-primitives, neo-wallets
- **L7 neo-application-logs** [exists] — Execution-log capture at commit + queryable JSON persistence. Leaf consumer.  
  deps: neo-node-traits, neo-smart-contract, neo-storage, neo-ledger, neo-vm
- **L7 neo-consensus** [exists] — dBFT 2.0 state machine + message set + ConsensusSigner seam. Owns dBFT wire encoding (signature-invocation). Drops upward neo-core dep (ScriptBuilder now in neo-vm-rs).  
  deps: neo-p2p, neo-primitives, neo-crypto, neo-io, neo-vm-rs, neo-node-traits
- **L7 neo-hsm** [exists] — Hardware-backed signer (Ledger/PKCS#11/sim) over HsmSigner trait; feature-gated. Uses shared neo-crypto redeem-script helper.  
  deps: neo-crypto, neo-primitives
- **L7 neo-oracle-service** [exists] — Off-chain Oracle node plugin (HTTPS/NeoFS fetch, response-tx sign/relay). Leaf consumer.  
  deps: neo-node-traits, neo-smart-contract, neo-native-contracts, neo-p2p, neo-wallets, neo-storage, neo-crypto, neo-json
- **L7 neo-rpc** [merge-target] — JSON-RPC server (HTTP/WS endpoints + method handlers) over a live node. Depends on neo-rpc-types; no longer feature-entangled with the client. RPC metrics owned by neo-telemetry.  
  deps: neo-rpc-types, neo-p2p, neo-ledger, neo-smart-contract, neo-native-contracts, neo-node-traits, neo-telemetry
- **L7 neo-rpc-client** [new] — Typed RPC client/SDK (reqwest). Depends only on neo-rpc-types + tx-builder/primitives; buildable without the server stack.  
  deps: neo-rpc-types, neo-tx-builder, neo-primitives, neo-crypto
- **L7 neo-rpc-types** [new] — Shared RPC wire DTOs (RpcBlock/RpcTransaction/RpcInvokeResult/RpcNep17*/RpcNep11*/RpcContractState/error_code/wire RpcError) + param-converter enums. The seam both server and client serialize through.  
  deps: neo-p2p, neo-primitives, neo-json
- **L7 neo-state-service** [new] — StateService plugin: MPT state-root computation/persistence + validator-signed roots. Leaf consumer; unblocked by neo-node-traits seam.  
  deps: neo-node-traits, neo-smart-contract, neo-storage, neo-ledger, neo-p2p, neo-wallets
- **L7 neo-tee** [exists] — SGX/TEE sealed-key signer + fair-ordering mempool; feature-gated. Uses shared neo-crypto redeem-script helper.  
  deps: neo-crypto, neo-primitives
- **L7 neo-tokens-tracker** [exists] — NEP-11/17 balance+transfer indexer. TokensTrackerService promoted to the query API so neo-rpc stops importing internals.  
  deps: neo-node-traits, neo-smart-contract, neo-storage, neo-ledger, neo-vm-rs
- **L8 neo-core** [delete] — DELETED after B14. All responsibilities extracted; residual facade/shim modules (persistence, protocol_settings, vm_runtime, hardfork) removed once consumers repoint.  
  deps: —
- **L8 neo-system** [new] — Top assembly: wires Blockchain + LocalNode + TaskManager + services; impls neo-node-traits::SystemContext. The single deliberately-fat orchestration crate.  
  deps: neo-ledger, neo-network, neo-smart-contract, neo-native-contracts, neo-node-traits, neo-config, neo-storage, neo-actors
- **L9 neo-node** [exists] — Runnable binary: CLI/TOML config, storage select, subsystem assembly, lifecycle. Owns operator config (NodeConfig sections) and the ConsensusActor bridge.  
  deps: neo-system, neo-consensus, neo-rpc, neo-oracle-service, neo-state-service, neo-tokens-tracker, neo-application-logs, neo-telemetry, neo-storage-rocksdb

## Overlap resolutions (the user's 'no functionality overlap' requirement)
- neo-rpc bloat #1 (server feature force-enables client to reach models): extract client/models/* + param-converter enums + wire RpcError into new leaf neo-rpc-types; both server and client depend on it; drop server=[client].
- neo-rpc bloat #2 (client SDK depends on neo-core monolith for tx-building): after dissolution, neo-rpc-client depends only on neo-rpc-types + neo-tx-builder + neo-primitives, never on neo-system/runtime. Split neo-rpc into neo-rpc (server) + neo-rpc-client.
- config/RPC dup: DELETE neo-config::RpcSettings (dead, zero external consumers); neo-rpc::RpcServerConfig is the single source of truth neo-node already loads.
- config/protocol dup: already resolved (A3) — neo-config owns canonical ProtocolSettings; neo-core protocol_settings.rs is a shim to delete at B14.
- config dead half: DELETE neo-config Settings/NodeSettings/NetworkConfig/StorageSettings/LoggingSettings/TelemetrySettings/ConsensusSettings (zero consumers; duplicated by neo-node/src/config/sections.rs which is the live loader). Rescope neo-config to chainspec (ProtocolSettings+Hardfork+NetworkType+GenesisConfig).
- storage layering: relocate RocksDB-specific StorageConfig/CompactionStrategy/CompressionAlgorithm + read_cache/bloom/prefetch OUT of generic neo-storage INTO neo-storage-rocksdb; make neo-config (chainspec) not own storage knobs. Keeps neo-storage a clean trait+types+memory crate.
- storage CompressionAlgorithm split-brain: neo-storage named an LZ4 enum it never performs while neo-io owns the impl. After RocksDB config moves to neo-storage-rocksdb and LZ4 consolidates in neo-io, single owner per concern.
- neo-io over-scope: MOVE caching/ (HashSetCache/RelayCache/InventoryHash) to neo-network (its only consumers); DELETE production-dead FIFOCache/IoCache. neo-io becomes binary-IO + LZ4 only.
- telemetry dup #1 (two metric registries): delete the instance-based Metrics/MetricsServer; keep the node_metrics global path; remove the stub MetricsServer::start.
- telemetry dup #2 (RPC metric identities in both neo-rpc and neo-telemetry): move neo_rpc_requests_total/neo_rpc_errors_total ownership into neo-telemetry; neo-rpc calls update fns; drop prometheus dep from neo-rpc.
- telemetry dup #3 (logging init duplicated in neo-node): delete neo-node logging::init_tracing + startup/logging.rs; route through neo_telemetry::init_node_logging.
- tee/hsm/core triplicated primitive: push signature_redeem_script + script_hash_from_public_key + neo_address into neo-crypto; neo-core(Helper/Contract), neo-hsm, neo-tee all consume the single low-level helper.
- neo-json escape dup (consensus-critical): make neo-core/smart_contract/json_serializer.rs delegate to neo_json::escape::to_vec; delete its private write_json_string/write_json_value; fixes the quote-escape divergence and unifies manifest+RPC escape bytes.
- neo-primitives fake -api seam: DELETE dead BlockchainProvider/PeerRegistry/HeaderLike/TransactionLike/PeerId/primitives::PeerInfo/RelayError/BlockchainSnapshot + VerificationContext trait collisions (only referenced by their own tests); move load-bearing Witness/StorageValue/BlockLike traits into the proper seam crates (neo-p2p verification / neo-storage).
- neo-p2p dead abstraction: DELETE traits.rs (PeerManager/Broadcaster/DataRequester/P2PService/P2PEvent/P2PConfig/PeerInfo — zero impls/consumers) and the pure pass-through re-export modules (oracle_response_code/transaction_removal_reason/witness_condition_type/witness_rule_action); live seam is neo-core's PeerManagerService.
- tokens-tracker read/write split across boundary: promote TokensTrackerService into the query API (getnep17/11 balances+transfers) so neo-rpc stops importing Nep17Tracker::rpc_prefixes()/raw keys/find_range; single owner of prefix bytes + key encoding.
- neo-vm rpc_json presentation concern: optionally relocate neo-vm/src/rpc_json.rs to a neo-rpc-side adapter (its only consumer) — low priority, non-blocking.
- consensus upward dep: drop neo-consensus's neo-core dependency entirely once ScriptBuilder lives in neo-vm-rs; consolidate the 66-byte signature-invocation encode/parse here so neo-node consumes it.

## neo-core module → target crate mapping
- `smart_contract/ (engine, interop, manifest/ABI/NEF, Contract model, serializers)` → **neo-smart-contract** — ApplicationEngine host + interop + Contract/ContractParameter; implements VerificationContext+EngineHost. StackItem serializers already live in neo-vm. Gated on A4+A6.
- `smart_contract/native/ (11 contracts)` → **neo-native-contracts** — Split out ONLY after A6 NativeContract/EngineHost seam lands; injected via NativeRegistry. Largest consensus-critical seam.
- `smart_contract NativeContract trait + EngineHost` → **neo-native-traits** — Hoist the trait + registry to a tiny seam crate so engine never imports native bodies (dependency inversion).
- `network/p2p/payloads/ (14 heavyweight chain types) + witness.rs + witness_rule/ + witness_rule.rs` → **neo-p2p** — Move 14 files into existing neo-p2p; collapse duplicate ledger/block.rs into payloads/block.rs (diff serialize() byte-for-byte). Delete the stray top-level witness_rule.rs (dir is canonical). Gated on A4/S3.
- `network/ transport (local_node, remote_node, task_manager, messages, framing)` → **neo-network** — New crate above neo-p2p; actors hold Arc<dyn SystemContext> after A5. Relay/inventory caches arrive from neo-io::caching.
- `ledger/ (Blockchain actor, MemoryPool, genesis)` → **neo-ledger** — Pure data types (HeaderCache/TxVerCtx/VerifyResult/LedgerContext/duplicate Block) move DOWN to neo-p2p first; actor stays here, holds Arc<dyn SystemContext>.
- `neo_system/ (NeoSystem assembly + NeoSystemContext)` → **neo-system** — Top crate; impls neo-node-traits::SystemContext. Near-last extraction (B13).
- `state_service/` → **neo-state-service** — New plugin crate; unblocked once neo-node-traits + payloads/native land. Was blocked on NeoSystem.state_store cycle.
- `wallets/` → **neo-wallets** — Accept forward edge to neo-smart-contract (Q3). Unblocks after B5+B6.
- `persistence/ (re-export shim)` → **neo-storage** — Already a shim with zero back-edges; repoint ~265 DataCache/StoreCache refs to neo_storage:: at B14 and delete the shim.
- `services/traits.rs (SystemContext + service traits)` → **neo-node-traits** — Promote verbatim; concrete LockedMempoolService moves to neo-system. GATE B9 — return types (StoreCache/Block) must be in low crates first.
- `events/ (event-handler traits, i_event_handlers)` → **neo-node-traits** — Move alongside SystemContext; replace &dyn Any with &dyn SystemContext. Arg types (Block/Message/Wallet) already in low crates by then.
- `builders/` → **neo-tx-builder** — Pure lift above neo-p2p once payloads land (B5). No seam.
- `extensions/ (SerializableExt, byte/span/memory traits)` → **neo-io** — Fold IO extension traits into neo-io; map CoreError->IoError. SAFE leaf lift.
- `compression/ (LZ4 wrappers)` → **neo-io** — Zero crate:: edges; fold into neo-io with extensions. Wire-relevant (ExtensiblePayload) — keep byte-for-byte. SAFE now.
- `protocol_settings.rs (7-LOC re-export shim)` → **neo-config** — Canonical already in neo-config (A3 done); repoint ~70 ext + ~56 int refs to neo_config:: and delete shim at B14. SAFE incremental.
- `script_builder.rs + script_validation.rs` → **neo-vm-rs** — Pure bytecode emit/validate over neo-vm-rs metadata, no core edges. Co-locate with neo-vm-rs (which owns OpCode). Also severs neo-consensus's upward neo-core dep. SAFE now.
- `vm_runtime.rs (27-LOC alias)` → **neo-vm** — Pure alias over already-extracted neo-vm; repoint callers to neo_vm:: and delete at B14. SAFE.
- `validation.rs (block/tx security checks)` → **neo-ledger** — Block-assembly checks to neo-ledger, type-level checks to neo-p2p. Needs B5 (payload types) first.
- `constants.rs + time_provider.rs` → **neo-primitives** — Clean leaf lifts to neo-primitives. SAFE now.
- `hardfork.rs (re-export shim)` → **neo-primitives** — Hardfork already moved (A3); delete shim at B14.
- `error.rs (CoreError/CoreResult) + macros.rs` → **neo-primitives** — DO NOT lift whole. Decompose per-extraction: each crate gets its own error (VmError/StorageError already exist); map variants outward as modules leave. Shared macros distribute with their users.
- `prelude.rs / properties` → **neo-core** — Dissolve with the facade at B14; prelude re-exports repoint to leaf crates.

## Redundant / rescoped crates
- neo-core — DELETE after full extraction (B14); it is the monolith being dissolved, not a kept crate. Residual shims (persistence, protocol_settings.rs, vm_runtime.rs, hardfork.rs) removed with it.
- neo-config node-operator-settings half — DELETE the modules (Settings/NodeSettings/NetworkConfig/RpcSettings/StorageSettings/LoggingSettings/TelemetrySettings/ConsensusSettings); dead, duplicated by neo-node/src/config/sections.rs. The crate survives, rescoped to chainspec.
- neo-primitives blockchain/ + verification trait seams — DELETE dead types (BlockchainProvider/PeerRegistry/HeaderLike/TransactionLike/PeerId/PeerInfo/RelayError/BlockchainSnapshot/VerificationContext-trait); ~150+ LOC, no implementors/consumers.
- neo-p2p traits.rs — DELETE entirely (262 LOC of unimplemented P2PService/PeerManager speculative API) plus pass-through re-export modules.
- neo-io caching FIFOCache/IoCache/FifoEntries — DELETE (production-dead, only tests reference); relocate the live HashSetCache/RelayCache/InventoryHash to neo-network.
- neo-crypto crypto_utils.rs — DELETE compat re-export shim (redundant dual access paths); repoint ~5 callers to crate-root re-exports.
- neo-telemetry instance Metrics + MetricsServer — DELETE (unwired duplicate of node_metrics global path; MetricsServer::start is a stub). No crate deleted, dead code removed.
- NOT redundant (explicitly keep): neo-vm vs neo-vm-rs (deliberate two-tier), neo-actors, neo-hsm vs neo-tee (disjoint trust models/deps), neo-json, neo-storage-rocksdb, all four plugin crates.

## Execution sequence (with progress)
1. [x] DONE 2026-05-31 **[SAFE-NOW]** DELETE neo-config dead settings half (Settings/Node/Rpc/Storage/Logging/Telemetry/Consensus); rescope to chainspec. Verified zero external consumers.
2. [x] DONE 2026-05-31 **[SAFE-NOW]** DELETE neo-primitives dead blockchain/ trait seams + VerificationContext-trait/PeerInfo collisions (only self-test refs).
3. [~] PARTIAL (traits.rs done; neo-io FIFOCache/IoCache pending) **[SAFE-NOW]** DELETE neo-p2p traits.rs + pass-through re-export modules; DELETE neo-io dead FIFOCache/IoCache; DELETE neo-crypto crypto_utils shim; DELETE neo-telemetry instance Metrics/MetricsServer stub.
4. [ ] **[SAFE-NOW]** Move ScriptBuilder + script_validation into neo-vm-rs; drop neo-consensus upward neo-core dep.
5. [ ] **[SAFE-NOW]** Fold neo-core compression/ (LZ4) + extensions/ (SerializableExt) into neo-io; lift constants.rs + time_provider.rs into neo-primitives. Move neo-io caching/ live types to where neo-network will be (stage in neo-p2p or hold).
6. [ ] **[SAFE-NOW]** Consolidate the Neo redeem-script/script-hash primitive into neo-crypto; repoint neo-core Helper/Contract, neo-hsm, neo-tee. Make neo-core json_serializer delegate to neo_json::escape.
7. [ ] **[SAFE-NOW]** Relocate RocksDB StorageConfig/Compaction/read_cache from neo-storage into neo-storage-rocksdb; dedup against (now-deleted) config; neo-config no longer owns storage knobs.
8. [ ] **[SAFE-NOW]** Extract neo-rpc-types (shared DTOs) from neo-rpc/client/models; break server=[client] feature entanglement.
9. [ ] **[GATED]** A4/S3 — wire VerificationContext + BlockchainSnapshot + NativeQueries seam; move Oracle/Role domain types to a low crate; route all 5 attribute verify() through the trait; update Transaction::verify caller. Differential-test on consensus path.
10. [ ] **[GATED]** A5/S7+S8 — invert blockchain/mod.rs + task_manager to Arc<dyn SystemContext> in-place; replace &dyn Any in event handlers. Audit return types for upward concrete leaks.
11. [ ] **[GATED]** A6/S6 — introduce NativeContract/EngineHost seam (207 sites); engine holds injected Vec<Arc<dyn NativeContract>>. Keep engine+native co-located until this lands.
12. [ ] **[GATED]** B5 — grow neo-p2p with 14 chain-type files + witness/witness_rule + HeaderCache/TxVerCtx/VerifyResult/LedgerContext; collapse duplicate Block (byte-for-byte serialize() diff first).
13. [ ] **[GATED]** B6 — extract neo-smart-contract (engine framework, implements VerificationContext+EngineHost); B4 neo-vm already done.
14. [ ] **[GATED]** B7 — extract neo-native-contracts (11 contracts) via NativeRegistry injection.
15. [ ] **[GATED]** B8 — extract neo-wallets (forward edge to neo-smart-contract) + neo-tx-builder.
16. [ ] **[GATED]** B9 — promote services/traits.rs + events/ into neo-node-traits (return types now in low crates).
17. [ ] **[GATED]** B10/B11 — extract neo-ledger (actor+mempool+genesis, validation.rs split) and neo-network (transport, caching folded in).
18. [ ] **[GATED]** B12 — extract plugins in parallel: neo-state-service, plus repoint existing neo-oracle-service/neo-tokens-tracker/neo-application-logs to neo-node-traits; promote TokensTrackerService query API; split neo-rpc-client out.
19. [ ] **[GATED]** B13 — extract neo-system (neo_system/ → top crate, impls SystemContext).
20. [ ] **[GATED]** B14 — repoint neo-node/tests/neo-rpc/neo-consensus off neo-core; delete residual shims (persistence/protocol_settings/vm_runtime/hardfork); DELETE neo-core.
