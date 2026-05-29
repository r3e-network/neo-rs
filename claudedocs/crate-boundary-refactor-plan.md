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
neo-plugins extraction (application_logs/oracle_service), plugin host, console REPL — standalone follow-ups, none blocks the boundary refactor.

---

## VERIFIED architectural realities (hands-on, correcting the audit's optimism)

The synthesis flagged most moves "VERIFY-FIRST"; verification shows several are
**not** mechanical and a few would create dependency cycles. Recorded so the
roadmap is accurate and future work doesn't repeat the analysis:

- **`Designate` native contract is NOT missing** — it is present as
  `RoleManagement` (neo-core/src/smart_contract/native/role_management.rs, id -8,
  hash 0x49cf…95e2). Neo N3 renamed Designate→RoleManagement. Audit false positive.
- **P2P actors → neo-p2p creates a CYCLE.** The actors (local_node/remote_node/
  task_manager) use both `Arc<NeoSystemContext>` AND neo-core payload types
  (Block/Transaction/Message). neo-core already depends on neo-p2p (Cargo.toml),
  so a reverse edge = cycle; a `NeoSystemContext` trait seam alone does not fix
  the payload coupling. **Correct interpretation:** keep neo-p2p as the
  low-level wire-types crate (below neo-core); the runtime actors are
  orchestration that belongs in the runtime layer (neo-core `runtime`-gated, as
  today, or neo-node) — NOT in low-level neo-p2p. The real P2P boundary fix is
  **overlap elimination** of the wire types (see below), not an actor move.
- **`neo_system` → neo-node also risks a cycle.** neo-consensus and neo-rpc
  consume `NeoSystemContext`; both are below neo-node. Moving it up requires a
  trait seam those crates depend on instead of the concrete type. Not a move.
- **Wallet adapters → neo-hsm/neo-tee need decoupling first.** `hsm_wallet`
  holds `runtime: HsmRuntime`, and `HsmRuntime` depends on `crate::cli::NodeCli`
  (neo-node). Moving the wallet requires either moving/abstracting HsmRuntime or
  refactoring the adapter to hold the `neo_hsm::HsmSigner` directly. (The shared
  `signature_invocation` helper has been moved to neo-core/wallets — done.)
- **ProtocolSettings is layered, not duplicated.** neo-config::ProtocolSettings
  (TOML schema: ms_per_block:u64, String validators) and
  neo-core::ProtocolSettings (runtime: milliseconds_per_block:u32, ECPoint
  standby_committee, hardfork map, hardcoded mainnet/testnet keys) are different
  layers with different field names/types — not a byte-dup. Consolidating is a
  consensus-adjacent conversion-layer task (defaults feed genesis), not a merge.
- **P2P wire-type "duplication"** (message_command/message_flags in neo-core vs
  neo-p2p) is the same `neo_primitives::p2p_message_command!` /
  `protocol_message_flags!` macro instantiated with different error types
  (neo-core `NetworkError` vs neo-p2p `P2PError`). Consolidating means unifying
  the networking error model — fiddly, low payoff, touches P2P infra.

**Net:** decomposing the neo-core monolith further is a major trait-abstraction
effort (neo-core is the integration hub, like C# Neo.dll), not file moves, and
must be staged with the CONSENSUS GUARD. It needs a maintainer decision on the
neo-p2p layering direction before the largest moves are attempted.

## Completed safe boundary work (verified, committed)
- **VM migration epic** (prior): deleted local neo-vm crate; VM host → neo-core;
  BinarySerializer/JsonSerializer/NotifyEventArgs/StorageContext/ScriptBuilder →
  correct layers; WitnessRule/CallFlags/MethodToken out of foundation crates;
  StackValue projections — byte-identical, full workspace 3455 tests green.
- **neo-config quality**: socket-addr accessors → `Result`; genesis panic removed.
- **wallets**: `signature_invocation` helper relocated neo-node → neo-core/wallets.
