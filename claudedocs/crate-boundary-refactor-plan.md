# neo-rs Crate-Boundary Refactor Plan (v2 — post-VM-migration audit)

Parity target: C# Neo N3 v3.9.1/3.9.2.
Layering invariant: `primitives < crypto < io/json < storage < core < {p2p, rpc, consensus} < node`.
Source: 17-analyst parallel audit + synthesis (workflow `wf_7ea9e808-d12`, 2.4M tokens).

> 🚨 **CONSENSUS GUARD** — runs at the end of EVERY wave; any byte drift = stop & revert:
> - genesis block-hash KAT (`neo-core/src/ledger/genesis.rs`)
> - `neo-core/tests/block_serialization_compatibility_tests.rs`
> - native-contract serialization round-trips (`native_contract_tests`, `native_token_tests`)
> - `neo-core/tests/block_sync_tests.rs`
> Plus `cargo build --workspace` green and no test regression.

## Ground-truth corrections (verified)
- **P2P infra → neo-p2p is blocked on a trait seam.** `remote_node.rs:56`, `task_manager.rs:103`, `local_node` hold `Arc<NeoSystemContext>` (neo-core, runtime-gated). neo-p2p has **zero** neo-core deps; a naive move = cycle. Need M7 (trait in neo-primitives) first.
- **consensus.rs → neo-consensus** is directionally legal (neo-consensus already deps neo-core) but couples to actor runtime + wallets + native contracts. Split, don't bulk-move.

## Boundary verdicts
clean: neo-primitives, neo-crypto, neo-io, neo-json, neo-storage, neo-p2p, neo-rpc, neo-tee, neo-hsm, neo-telemetry, neo-consensus.
needs-refactor: **neo-config** (ProtocolSettings overlap), **neo-core** (P2P infra + neo_system misplaced), **neo-node** (4 misplaced impls).

## Execution waves
- **Wave 0 — shim/dead-code removal** (risk none): delete `network/p2p/channels_config.rs`+`timeouts.rs` shims → `neo_p2p`; remove `neo_io` inline shim (`lib.rs:320`, task #11); drop `config` re-export (VERIFY no external consumer).
- **Wave 1 — wallet adapters out of daemon** (none): M1 `neo-node/src/hsm_wallet.rs`→neo-hsm; M2 `tee_wallet.rs`→neo-tee; fix neo-hsm stale `HsmRuntime` docstring; unwrap→Result during move.
- **Wave 2 — neo-config quality + ProtocolSettings consolidation** (medium): `settings.rs:472/480` expect→Result; `genesis.rs:244` unwrap; neo-config=schema, neo-core wraps; ECPoint parse for standby_validators. GUARD mandatory (feeds genesis).
- **Wave 3 — P2P infra → neo-p2p** (low, big): M7 trait seam in neo-primitives → M8 move connection/framed/peer/local_node/remote_node/task_manager (~5200 LOC); M5 MessageCommand/Flags → neo-primitives re-export (VERIFY-FIRST). Payloads STAY in neo-core.
- **Wave 4 — neo_system → neo-node + import_acc → neo-core** (none, large): M9, M6.
- **Wave 5 — consensus state machine → neo-consensus** (HIGH, last): M10 split `consensus.rs` pure state-machine → neo-consensus, wiring stays in neo-node; definition-only god-module splits (stack_item.rs, bonus.rs, contract_manifest.rs, services.rs).

## Overlap to eliminate
- `ProtocolSettings` dup: neo-config/src/protocol.rs ↔ neo-core/src/protocol_settings.rs → split-role (Wave 2).
- `neo_io` inline shim (Wave 0). MessageCommand/Flags wrappers (Wave 3, M5).
- telemetry split (neo-core/src/telemetry vs neo-telemetry) = intentional, no action.

## Gaps vs C# v3.9.x (follow-ups, not blocking)
- medium: Designate native contract; ECPoint standby_validators; StorageContext system iface; plugin system.
- low: hardfork sequence validation; SnapshotCache wrapper; UPnP stub; console REPL.

## Deferred / out of scope
neo-plugins extraction (application_logs/oracle_service), Designate contract, plugin host, console REPL — standalone follow-ups, none blocks the boundary refactor.
