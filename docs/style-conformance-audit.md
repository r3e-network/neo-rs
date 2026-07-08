# Style Conformance Audit

This document turns the project coding guidance into a repeatable audit plan.
It is intentionally stricter than ordinary formatting review: the goal is for
every crate to express Neo domain intent at the top layer, keep mechanics in the
right lower layer, and use Rust's type system where it improves correctness and
speed.

The canonical neo-rs rules are in
[`coding-design-architecture-guidance.md`](coding-design-architecture-guidance.md).
The user also pointed to Apollo's
[`rust-best-practices`](https://github.com/apollographql/rust-best-practices)
handbook. We use that handbook as supporting Rust guidance, pinned at revision
`8425b336d368edaddbab8a5339030c677d63dc5d` when this audit was written,
especially for:

- borrowing before cloning and avoiding early allocation;
- precise `Option` / `Result` handling instead of production panic paths;
- clippy discipline and narrow, justified `#[allow]` sites;
- measured performance work;
- crate-level typed errors and `anyhow` mostly for binaries;
- tests as living documentation;
- static dispatch for known/hot collaborators and `dyn Trait` only for real
  runtime polymorphism;
- type-state only when it prevents invalid workflow states without adding
  ceremony.

## Review Checklist

Use this checklist for every crate, file, and trait review.

- **Layer vocabulary:** a function should stay at one abstraction level.
  Startup, RPC handlers, consensus, storage, VM, and MPT code should not expose
  each other's mechanics in the same public workflow.
- **Workflow readability:** top-level flows should read as ordered Neo domain
  operations and return named reports/handles, not unrelated primitives.
- **Trait boundaries:** use concrete types, generics, or associated types when
  production collaborators are known or hot. Keep `dyn Trait` for plugin
  registries, backend selection, wallets, RPC-only sources, mixed collections,
  or other runtime-polymorphic seams.
- **Typed boundaries:** decode `serde_json::Value`, raw VM stack values, byte
  maps, and primitive tuples at the boundary that understands them. Repeated
  shapes should become named structs/enums.
- **Errors:** library crates should expose typed errors or `CoreError`.
  `anyhow` is acceptable in binaries, CLI orchestration, scripts, and tests.
- **No production panics:** `unwrap` / `expect` / `panic!` in production code
  must be replaced with typed errors unless they document an impossible
  invariant and cannot fail in practice.
- **Allocation discipline:** prefer borrowing, slices, lazy fallbacks, and
  streaming/visitor APIs in sync, storage, VM, and RPC hot paths.
- **Clone discipline:** classify clones in hot paths as keep, borrow, snapshot,
  share, or measure. Large collection and byte-buffer clones in loops require a
  concrete reason.
- **Eager allocation:** replace eager `ok_or`, `unwrap_or`, `map_or`, and
  `or` fallbacks when fallback construction allocates or performs work.
- **Module organization:** crate roots are maps. Implementation belongs in
  domain folders; avoid root-level piles of `*_helper`, `*_utils`, `manager`,
  `runner`, or broad `context` files.
- **Naming:** names should teach the domain operation. Weak verbs such as
  `process`, `handle`, `do_work`, `manager`, and `helper` need a local domain
  noun or narrower module.
- **Comments and docs:** comments explain why; public docs explain contracts,
  side effects, layer ownership, and C# parity when relevant.
- **TODO hygiene:** bare TODOs are not durable documentation. Link them to a
  tracked issue or convert them into tests, types, or docs.
- **Performance evidence:** optimization patches need release-profile evidence:
  a benchmark, flamegraph, sync-speed probe, or measured allocation change.
- **Runtime-data hygiene:** committed files must be source, docs, scripts, or
  small deterministic fixtures with an explicit test purpose.

## Repeatable Scan

Run:

```bash
bash tools/quality/style-audit.sh > /tmp/neo-rs-style-audit.md
```

The script scans production Rust files for:

- dynamic dispatch sites;
- raw JSON boundary sites;
- production `unwrap` / `expect`;
- lint `allow` / `expect`;
- production `panic!`, `todo!`, and `unimplemented!`;
- clone sites and eager fallback sites that may allocate early;
- TODO/FIXME comment sites;
- trait definition sites;
- largest files;
- broad helper/utils/context paths;
- tracked runtime-data risk.

The script is intentionally a heuristic. A match is not automatically a bug:
for example, `Arc<dyn Store>` can be a legitimate runtime backend boundary, and
`serde_json::Value` is expected at the JSON-RPC transport edge. Reviewers must
classify each match as one of:

- **keep:** the boundary is real and documented;
- **type:** replace raw/dynamic values with a named domain type;
- **generic:** replace hot/known `dyn` use with a generic or associated type;
- **extract:** move mechanics into a lower layer/domain folder;
- **error:** replace panic/unwrap with typed error propagation;
- **measure:** benchmark before changing a performance-sensitive path.

## Current Baseline

Initial scan coverage: 28 workspace members plus support crates, with roughly
1,378 Rust files outside ignored build folders.

High-signal clusters found during the first pass:

- `neo-manifest/src/manifest/contract_manifest.rs` has been decomposed into
  root/domain, `json`, `stack`, `wire`, `validation`, and typed `fields`
  modules. The remaining manifest cleanup is to keep permission/trust policy
  readable at the domain layer.
- `neo-manifest` protocol types still depend on VM/runtime projection details
  (`neo_vm::Interoperable`, `neo_vm_rs::StackValue`). Core manifest models
  should keep stack adapters out of top-level domain flow.
- `neo-manifest/src/nef/nef_file.rs` now uses shared fallible NEF wire-writing
  helpers for checksum and byte serialization. The compatibility wrappers
  remain for existing callers, but the protocol writer no longer uses
  production `expect()` calls. The stack-projection adapters now expose or use
  typed fallible conversions as well, so the style audit no longer reports
  `neo-manifest` in the production unwrap/expect section.
- `neo-config/src/settings/protocol.rs` is now a typed settings facade. Built-in
  network presets live in `settings/protocol/presets.rs`, file/stream loading
  lives in `settings/protocol/load.rs`, JSON/raw config parsing lives in
  `settings/protocol/parse.rs`, and hardfork sequence rules live in
  `settings/protocol/validation.rs`.
- `neo-network/src/remote_node/session.rs` owns per-peer P2P protocol state.
  Explicit block-range fetch completion now treats missing pending state as a
  logged session invariant breach instead of panicking the remote-node task.
- `neo-node/src/node/chain_acc/mod.rs` and `neo-node/src/node/fast_sync/mod.rs`
  are very large workflow modules. They should be split into domain files such
  as `format`, `reader`, `import`, `report`, `package`, `manifest`, and
  `workflow` while keeping behavior locked by tests.
- `neo-node/src/node/mod.rs` is now the daemon module map and public `run()`
  facade. `node/daemon.rs` owns the top-level parse, config load, preflight,
  node composition, startup-import, live-service, and shutdown sequence. Startup
  preflight lives in `node/preflight.rs`, which owns config/storage preflight
  checks, remote-ledger preflight skips, operator messages, and the explicit
  continue/exit outcome. Startup import orchestration lives in
  `node/startup_import.rs`, which owns chain.acc and fast-sync import sequencing,
  stop-height handling, durable-mode restore, task abortion, and observability
  error reporting while the lower `chain_acc` and `fast_sync` modules retain
  package/import mechanics. Live service startup has also been moved out:
  `node/live_services.rs` owns telemetry metrics, P2P listener startup, seed
  dialing, RPC startup/keepalive, and observability heartbeat task spawning.
  Shutdown coordination now lives in `node/shutdown_flow.rs`, which owns signal
  outcome handling, observability reporting for signal failures, task
  cancellation/abort grace, state-service flush, and durable-store restoration.
  Node composition now carries the dBFT validator handle through the same
  startup tuple as the consensus setup instead of recovering it with `expect`.
- `neo-node/src/node/logging/mod.rs` is now a facade for logging setup:
  `logging/filter.rs` owns `RUST_LOG` / TOML directive selection,
  `logging/format.rs` owns operator-facing format parsing, and
  `logging/rotation.rs` owns file writer construction plus size-based archive
  rotation.
- `neo-node/src/consensus/mod.rs` is now a facade for validator-node consensus
  wiring: `consensus/driver.rs` owns the dBFT driver task, round snapshots,
  recovery-log setup, and event routing; `payload.rs`, `proposal.rs`, `setup.rs`,
  and `hsm.rs` retain codec, proposal-policy, validator setup, and signer wiring
  responsibilities.
- `neo-node/src/state_root/mod.rs` is now a facade for StateService wiring:
  `state_root/codec.rs` owns the extensible payload envelope helpers,
  `state_root/setup.rs` owns StateValidator key resolution, and
  `state_root/driver.rs` owns the active vote/relay/persist task with explicit
  native-provider verification.
- `neo-node/src/node/services/mod.rs` now focuses on operational service
  composition. `services/state.rs` owns StateService MPT store and
  sync/async commit-handler construction, `services/read_side.rs` owns
  indexer, ApplicationLogs, and TokensTracker construction, while
  `services/store.rs` owns service-store opening, storage config inheritance,
  and fast-sync backend mode.
- `neo-node/src/node/sync_metrics/mod.rs` is now the metrics facade and module
  map. `sync_metrics/render.rs` owns the operator-facing Prometheus summary
  flow, `sync_metrics/families.rs` owns bounded-label metric-family renderers,
  and `sync_metrics/writer.rs` owns the small text-format label writers.
- `neo-node/src/node/context/mod.rs` keeps `DaemonContext` construction and
  node/service handles. `context/system_context.rs` owns the
  `SystemContext` trait implementation and store-commit policy, while
  `context/plugins.rs` owns catch-up-aware StateService, indexer,
  ApplicationLogs, and TokensTracker hook dispatch.
- `neo-blockchain/src/ledger/ledger_context.rs` owns the hot in-memory ledger
  cache. Its block/header LRU capacity now clamps zero to one with explicit
  `NonZeroUsize` handling, matching the cache contract without a production
  `expect`.
- `neo-rpc` has the largest raw JSON surface. Many `Value` uses are correct at
  the transport edge, but handler internals should move repeated request and
  response shapes into typed parameter/result modules.
- `neo-execution`, `neo-vm`, and `neo-native-contracts` contain legitimate C#
  parity and interop boundaries, but production `expect` sites and broad
  `dyn Any` escape hatches should be reviewed one by one and either converted
  to typed errors/contexts or documented as impossible invariants. Within
  `neo-execution/src/application_engine`, the root keeps the public
  `ApplicationEngine` facade and module map while `application_engine/host_state.rs`
  owns the private VM host wrapper, host syscall metadata, queued native-call
  record, and CoreError-to-VM-fault projection.
- `neo-execution/src/lib.rs` still re-exports internal implementation modules
  broadly. Keep crate-root exports to deliberate facade/domain types after
  downstream imports are mapped.
- `neo-native-contracts/src/lib.rs` is now a crate facade and module map.
  NEP helpers live in `nep`, shared storage-byte projection lives in
  `storage_encoding`, and native method metadata/invocation mechanics stay in
  the individual contract folders.
- `neo-native-contracts/src/neo_token/storage/mod.rs` keeps the NEO storage
  module map and re-exports. `storage/economics.rs` owns register-price,
  GAS-per-block, voter-count, voter-reward, and unclaimed-GAS calculation
  storage helpers; account, candidate, committee, key, point, and view codecs
  stay in their named storage modules.
- `neo-consensus/src/context/mod.rs` keeps the public `ConsensusContext` state
  shape and module map, while `context/construction.rs` owns fresh-round field
  defaults and `context/policy.rs` owns dBFT default policy constants and
  bounded-cache limits. Context replay-cache construction, expected witness-size
  accounting, and signature verification helpers now avoid production
  `unwrap` / `expect` calls; the style audit no longer reports
  `neo-consensus` in the production unwrap/expect section.
- `neo-consensus/src/messages/mod.rs` keeps the dBFT message module map and
  re-exports. `messages/payload.rs` owns the shared `ExtensiblePayload.Data`
  DBFT envelope and common message-byte helper, while each message module owns
  its body codec and validation rules. `messages/wire.rs` owns shared body
  wire helpers such as Neo `UInt256[]` var-int encoding so signed dBFT message
  bodies do not use panic-only in-memory writer calls.
- `neo-consensus/src/service/helpers/block.rs` owns consensus block field
  derivation. Header hash construction now assembles the unsigned header bytes
  directly from fixed-width little-endian fields, keeping protocol field order
  visible without panic-shaped in-memory writer calls. Block witness multi-sig
  construction delegates valid validator sets to `neo-vm`'s shared
  `RedeemScript` builder, with a non-panicking fallback that preserves the
  helper's legacy byte-returning API for invalid inputs.
- `neo-indexer/src/indexer/mod.rs` keeps the mutable projection struct and
  constructor, `indexer/commands.rs` owns public block/notification indexing
  commands, `indexer/apply.rs` owns prepared-record application into the
  in-memory maps, and `indexer/block.rs` owns canonical block and transaction
  materialization before records are applied.
- `neo-indexer/src/service/mod.rs` keeps the service facade, constructors, and
  backend diagnostics, `service/commands.rs` owns public indexing/revert
  commands, `service/mutation.rs` owns persistence-aware mutation and rollback
  mechanics, and `service/backend.rs` owns durable backend kind, diagnostic
  paths, mutation mode selection, and persistence dispatch.
- `neo-indexer/src/store/mod.rs` now keeps durable-store facade exports,
  key/record/status module wiring, and test-only key re-exports.
  `store/record_codec.rs` owns JSON serde for individual records,
  `store/record_read.rs` owns lookup, paging, and filtered reads,
  `store/record_write.rs` owns snapshot-to-record materialization and puts, and
  `store/lifecycle.rs` owns schema detection, legacy snapshot migration, full
  store writes, and delta writes.
- `neo-io/src/serializable/mod.rs` keeps the codec facade and module map,
  `serializable/traits.rs` owns the `Serializable` and extension traits, and
  `serializable/macros.rs` owns the declarative `impl_serializable!` helper.
- `neo-serialization/src/codec/json_serializer.rs` owns NeoVM stack-item JSON
  projection. C#-compatible JSON escaping now has a fallible typed-error path
  for `serialize_to_byte_array`, while the legacy byte-returning helper keeps a
  non-panicking compatibility wrapper.
- `neo-vm/src/script_builder/mod.rs` is being decomposed into focused VM script
  construction modules. `script_builder/error.rs` owns the typed builder error,
  `script_builder/invocation.rs` owns single-signature invocation script
  helpers, `script_builder/push.rs` owns value-to-push-instruction
  serialization, `script_builder/control.rs` owns control-flow/syscall
  emission, and `redeem_script.rs` remains responsible for verification script
  construction.
- `neo-vm/src/jump_table/mod.rs` keeps the opcode-family module map and facade
  exports. `jump_table/table.rs` owns the fixed handler array, unsafe hot-path
  table access, and invalid-opcode dispatch, `jump_table/variants.rs` owns
  default and hardfork-specific table construction, and `jump_table/shared.rs`
  owns C# stack-coercion helpers, execution-context guards, semantics-error
  conversion, and StackValue result projection shared by opcode-family modules.
- `neo-vm/src/execution_engine/mod.rs` keeps the VM state facade and module map.
  `execution_engine/host.rs` owns the unsafe raw-host-pointer bridge used for
  allocation-free interop callbacks, including the documented safety invariants
  and callback wrappers.
- `neo-payloads/src/transaction/mod.rs` keeps the public transaction record,
  constants, and module map. `transaction/core.rs` owns constructors, getters,
  setters, cached hash/size helpers, and fee math; `serialization.rs` owns Neo
  wire codecs; `traits.rs` owns stack, inventory, payload, default, and hash
  trait adapters; and `verification.rs` owns witness-verification and
  `Verifiable` container adapters.
- `neo-payloads/src/transaction_attribute/mod.rs` keeps the attribute enum,
  constructors, and generic attribute helpers, while `transaction_attribute/wire.rs`
  owns type-byte dispatch plus `Serializable`, `fees.rs` owns policy-backed
  network-fee calculation, and `json.rs` owns RPC/JSON projection.
- `neo-storage` exposes broad `dyn Store` / `dyn StoreSnapshot` boundaries.
  This is valid for backend selection, but hot loops should keep borrowed
  visitor APIs or concrete paths where possible.
- `neo-storage/src/core/key_builder.rs` now exposes fallible construction and
  mutation as the public path for storage-key assembly. The panicking
  convenience wrappers were removed because storage keys carry consensus byte
  layout, and overflow or invalid capacity must propagate as typed errors under
  `panic = "abort"`. The style audit no longer reports `neo-storage` in the
  production unwrap/expect section.
- `neo-mempool/src/pool/memory_pool.rs` keeps the public pool facade,
  admission workflow, block-persist callbacks, and event ordering, while
  `pool/state.rs` owns the private queue indexes, verification-context
  accounting, conflict helpers, oracle-response tracking, and C# parity rules
  used while the pool lock is held.
- `neo-state-service` has a few public erased compatibility surfaces such as
  `Verifier<C = Arc<dyn StateRootCalculator>>` and
  `StateStore::with_mpt_store(..., Arc<dyn Store>)`. Prefer concrete
  constructors as the primary surface and name erased constructors explicitly.
  `storage/root_cache.rs` now normalizes zero capacity through a panic-free
  `NonZeroUsize` helper while preserving the one-entry minimum. The MPT
  known-empty continuation test also documents the current provider behavior:
  raw-overlay-capable stores commit local-root records without opening trie or
  backing snapshots, matching the batched empty-block fast path.
  `storage/mpt_store.rs` now returns typed `MptError::InvalidOperation` values
  for lazy-trie and non-empty-batch invariants instead of using production
  `expect()`. `service/commit_handlers.rs` exposes fallible async-worker
  constructors, and `neo-node/src/node/services/state.rs` uses that path during
  node composition, so the style audit no longer reports `neo-state-service` in
  the production unwrap/expect section.
- `neo-gui` is outside the workspace, but GUI mutex poison handling is now
  centralized in `neo-gui/src/sync.rs`; shell, runtime, and screen modules use
  that helper instead of choosing per-call `unwrap` / `expect` / silent-ignore
  behavior.
- `neo-oracle-service` has the densest lint-allow count and several
  generated/NeoFS adapter modules. Allows should be narrowed or annotated.
  `neo-oracle-service/src/service/mod.rs` now keeps the public service facade,
  module map, and runtime field layout. `service/status.rs` owns lifecycle
  status encoding, `service/error.rs` owns typed service errors, and
  `service/task.rs` owns pending response-signature task state, while
  `service/cache.rs` owns request deduplication, finished-request expiry, URL
  admission checks, and monitoring counters. HTTPS and NeoFS HTTP client
  construction now return typed initialization errors instead of panicking during
  service startup. NeoFS wallet-connect signing now builds its byte envelope
  directly with the shared Neo var-int codec instead of panic-only in-memory
  writer calls, and response-signature queue invariants now report
  `OracleServiceError`. The NeoFS gRPC verification recursion now carries the
  already-validated origin header instead of re-reading it with `expect`, so the
  oracle-service production panic scan is clear.
- Existing git hygiene rules exclude local ledgers, RocksDB state,
  checkpoints, logs, and build outputs. A scan did not find obvious tracked
  chain.acc/RocksDB artifacts, but runtime-data checks should stay in CI.

## Remediation Plan

Do not rewrite the whole workspace at once. Apply this sequence crate by crate:

1. **Protect behavior:** add or identify focused tests around the workflow or
   trait boundary before changing it.
2. **Classify boundaries:** mark `dyn Trait` and raw JSON/stack/byte boundaries
   as keep/type/generic/extract/error/measure.
3. **Remove panics first:** convert production panic paths to typed errors
   where the failure can come from external state.
4. **Split large workflow files:** move one cohesive domain at a time into a
   folder; keep re-exports stable.
5. **Introduce named outcomes:** replace primitive workflow returns with
   reports only where callers need the added meaning.
6. **Replace hot dynamic dispatch:** use generics or associated types only when
   the production collaborator is known and type propagation remains local.
7. **Type repeated RPC shapes:** keep raw JSON at transport/compatibility edges,
   but decode early inside handlers.
8. **Measure hot-path changes:** run focused benchmarks or sync-speed probes
   before claiming throughput improvements.
9. **Verify:** run formatting, clippy/test slices, and any parity/performance
   checks relevant to the touched crate.

## First-Fix Candidates

The first patch landed from this audit was a correctness fix in
`neo-execution/src/contracts/contract_parameters_context.rs`: malformed
`parameters` and `signatures` JSON entries now fail with field-specific
`CoreError` messages instead of being silently dropped. Regression tests live in
`neo-execution/src/tests/contracts/contract_parameters_context.rs`.

The second patch added a fast-sync local durability proof gate in
`neo-node/src/node/fast_sync/mod.rs` and
`neo-node/src/node/chain_acc/mod.rs`: post-import completion now compares the
reported last imported `chain.acc` tip with the durable local ledger tip before
clearing the fast-sync marker. This is not full reference-node/state-root
validation; it only prevents marking a fast-sync package complete when the
local store did not durably reach the reported imported tip.

The fast-sync reference proof now follows that local durability gate with
optional reference RPC validation: it fetches and decodes the raw `getblock`
payload at the imported tip, compares the decoded block height and hash, and
compares `getstateroot` when a local StateService root is available.

The chain.acc cleanup has started: production `PendingChainAccBatch`,
`ChainAccImportComposition`, and `ChainAccImportReport` no longer carry
test-only convenience helpers, and the root module no longer imports driver
test helpers just for `tests.rs`. The driver reader convenience wrappers now
live in the chain.acc test module instead of production `driver.rs`, and the
format parser tests/fixtures now live in `chain_acc/format_tests.rs` instead of
inside production `format.rs`. The metrics projection tests and synthetic
hot-path fixture builder now live in `chain_acc/metrics_tests.rs`, leaving
`metrics.rs` focused on runtime progress and hot-metric projection.

The first `neo-rpc` typed-helper pass is in `rpc_server_state`: positional
StateService request parsing now lives in `rpc_server_state/request.rs`, and
`getstateheight` shares the same no-parameter request validation as other
zero-argument RPC methods. State-height, state-root, `getproof`/`verifyproof`
payload envelopes, `getstate` value envelopes, and `findstates` JSON
construction live in `rpc_server_state/response.rs`. State proof handlers and
the C# proof-payload codec now live in `rpc_server_state/proof.rs`.
`rpc_server_state/roots.rs` owns `getstateheight` / `getstateroot`
orchestration and MPT fallback reads. Historical trie lookup mechanics for
`getstate` / `findstates`, including root gating, storage-key construction, and
C#-compatible trie error mapping, now live in
`rpc_server_state/state_queries.rs`. `rpc_server_state/support.rs` owns
StateStore/MPT service lookup, `UnsupportedState` error construction, and the
`findstates` page cap. The handler module now owns only handler registration.
Parameter conversion now follows the same module-map rule:
`parameter_converter/scalar.rs` owns string, boolean, numeric, Base64 bytes,
and UUID `RpcConvertible` implementations. `parameter_converter/domain.rs`
owns address arrays, block hash/index identifiers, and contract
name/hash/id conversions. `parameter_converter/contract_parameters.rs` owns
contract-parameter array conversion. `parameter_converter/errors.rs` owns
shared `InvalidParams` construction, `parameter_converter/parsing.rs` owns
shared address and UInt160 text parsing, and `parameter_converter/tokens.rs` owns
generic `JToken` shape checks, numeric coercion, and serde JSON projection.
The root `parameter_converter/mod.rs` keeps the converter facade, context,
trait, module map, and helper imports for child modules.
The same split now covers token tracker handlers:
`rpc_server_tokens_tracker/request.rs` owns account/time-window/token-id
parsing, while `rpc_server_tokens_tracker/response.rs` owns balance, transfer
entry, token-entry, and transfer-history response envelopes. Handler mechanics
are now separated by endpoint family as well: `balances.rs` owns NEP-11/NEP-17
balance enrichment, `transfers.rs` owns transfer-history routing,
`helpers.rs` owns tracker service lookup and transfer range ordering, and
`properties.rs` owns the NEP-11 property VM invocation. The root token-tracker
module is now just the method registry plus module map.
Wallet cleanup has started with the same boundary: `rpc_server_wallet/request.rs`
now owns management and network-fee request decoding (`dumpprivkey`,
`getwalletbalance`, `importprivkey`, `openwallet`, `calculatenetworkfee`) and
no-parameter validation for close/list/new-address/unclaimed-GAS methods, so
the wallet root handler can focus on wallet orchestration, native balance
queries, and fee calculation. The same request module now also owns transfer,
`sendmany`, signer, and cancel-transaction parameter decoding, leaving
`transfers.rs` focused on descriptor lookup, amount conversion, transaction
construction, and transfer/cancel orchestration. `rpc_server_wallet/signing.rs`
owns C# `Wallet.Sign` parity, witness completion, network-fee adjustment, and
relay result projection. `rpc_server_wallet/lifecycle.rs` owns open/close,
address creation/listing, and WIF import/export orchestration.
`neo-wallets/src/bip32/extended_key.rs` owns BIP-32 key derivation. Child-key
HMAC output splitting now uses explicit fixed-array copies instead of
panic-shaped slice conversion, with a regression test pinning direct derivation
to path-based derivation.
`rpc_server_wallet/support.rs` owns shared wallet runtime helpers used across
lifecycle, transfer, signing, and fee paths, including wallet lookup, wallet
future bridging, address-version script-hash parsing, and signature-contract
pubkey extraction. `rpc_server_wallet/errors.rs` owns wallet-domain error
projection and transfer insufficient-funds compatibility mapping into RPC
exceptions, including the C# invalid-operation compatibility code shared by
error projection tests. The root wallet module now keeps only handler
registration.
`rpc_server_wallet/balance.rs` owns `getwalletbalance`,
`getwalletunclaimedgas`, and the native balance/unclaimed-GAS probe logic.
`rpc_server_wallet/network_fee.rs` owns `calculatenetworkfee` request execution
and wallet-account script projection, while `rpc_server_wallet/response.rs`
owns lifecycle success/string/account/list shapes plus balance, unclaimed-GAS,
and network-fee response envelopes.
`wallet_compat/errors.rs` owns C# wallet compatibility error vocabulary,
`wallet_compat/signing.rs` owns C# `Wallet.Helper.Sign` parity, and the
`wallet_compat` root now keeps only facade exports plus its module map.
ApplicationLogs now follows the same split: `rpc_server_application_logs/request.rs`
owns hash and trigger-filter parsing, `lookup.rs` owns ApplicationLogs service
lookup and block/transaction log retrieval, and `response.rs` owns the optional
trigger filtering over stored C#-compatible log JSON. The root module now keeps
only handler registration. Direct handler tests cover transaction-log lookup,
trigger filtering, unknown hashes, and missing service errors.
Oracle submission follows the request-boundary rule as well:
`rpc_server_oracle/request.rs` owns Base64 decoding, request-id parsing, and
secp256r1 public-key validation for `submitoracleresponse`; `submission.rs`
owns service lookup, submission, and `OracleServiceError` mapping, while
`rpc_server_oracle/response.rs` owns the empty success payload. The root module
now keeps only handler registration.
Utility endpoints now use the same pattern: `rpc_server_utilities/request.rs`
owns no-parameter validation for `listplugins` / `listservices` and
`validateaddress` parameter parsing, `rpc_server_utilities/address.rs` owns
address-version validation and `validateaddress` dispatch, while
`rpc_server_utilities/inventory.rs` owns `listplugins` / `listservices`
dispatch. The root utility module now keeps only handler registration.
`rpc_server_utilities/response.rs` owns the `listplugins` plugin-entry/list,
`listservices` service-entry/list, and `validateaddress` JSON shapes. The
no-parameter request record is now shared through `rpc_helpers::NoParamsRequest`
so endpoint families do not grow private copies of the same invalid-params
contract.
Node relay methods now follow the same boundary:
`rpc_server_node/request.rs` owns Base64 decoding and Neo wire-payload
deserialization for `sendrawtransaction` and `submitblock`;
`rpc_server_node/relay.rs` owns endpoint orchestration, while
`rpc_relay/result.rs` owns C#-compatible relay-result mapping for both node and
wallet submission paths, and `rpc_relay/runtime.rs` owns the synchronous bridge
into async blockchain service calls.
Node version reporting now follows the same endpoint-family split:
`rpc_server_node/version.rs` owns dynamic Policy storage readers and
remote-ledger version projection. `rpc_server_node/status.rs` owns
`getconnectioncount`, `getpeers`, and shared local-node projection for node
status/version handlers, while `rpc_server_node/response.rs` owns the
C#-compatible version, connection-count, and peer-list JSON shapes, including
hardfork/public-key formatting.
`rpc_server_node/request.rs` owns the shared no-parameter validation for
status/version methods and the Base64 wire-payload decoding for relay methods.
The root `rpc_server_node/mod.rs` is now only the handler-registration facade
and module map.
Blockchain storage methods now follow that request-boundary pattern:
`rpc_server_blockchain/request_helpers.rs` owns contract identifier and Base64
key/prefix/start parsing for `getcontractstate`, `getstorage`, and
`findstorage`, while `storage.rs` keeps contract resolution, storage lookup,
and pagination. `responses.rs` owns contract-state projection plus base64
storage values and `findstorage` page envelopes.
Blockchain mempool handling has been moved out of the large route map:
`rpc_server_blockchain/mempool.rs` owns `getrawmempool` live-pool reads and
ledger-height lookup, `request_helpers.rs` owns `shouldGetUnverified` parsing,
and `responses.rs` owns the verified-hash array and verbose mempool response
envelopes. `mod.rs` stays closer to registration plus remaining legacy groups.
Blockchain transaction lookup now follows the same split:
`rpc_server_blockchain/transactions.rs` owns `getrawtransaction` and
`gettransactionheight` pool/ledger lookup, `request_helpers.rs` owns hash and
verbose parsing, and `responses.rs` owns C#-compatible raw base64, transaction
height, and verbose transaction enrichment (`confirmations`, `blockhash`, and
`blocktime`) response shapes.
Blockchain native/governance queries have moved out of the route map:
`rpc_server_blockchain/native.rs` owns native contract listing, committee,
validator, and candidate query flow over `NativeQueries`, while
`responses.rs` owns native-contract, committee, validator, and candidate
response projection.
`native_queries/script.rs` owns the C# `EmitDynamicCall` bytecode layout for
read-only native probes; `native_queries/result.rs` owns NEO stack-result
decoding for committee, validator, and candidate probes;
`native_queries/execution.rs` owns read-only VM setup and HALT validation;
`native_queries/registry.rs` owns standard native-contract registry
construction; `native_queries/neo.rs` owns the NEO native-token read probes;
and `native_queries/mod.rs` keeps only the `NativeQueries` type and module map.
Blockchain block/header methods now follow the same route-facade split:
`rpc_server_blockchain/blocks.rs` owns best hash, block/header counts,
block-hash lookup, block/header retrieval, and block system-fee calculation,
while `request_helpers.rs` owns the typed height and block-payload request
records for `getblockhash`, `getblock`, `getblockheader`, and `getblocksysfee`,
and `responses.rs` owns the hash/count/base64/system-fee envelopes plus verbose
block/header JSON enrichment.
The same request-helper module re-exports `NoParamsRequest` so blockchain
status/native methods reject unexpected parameters before local lookup or
remote-ledger forwarding.
The root `rpc_server_blockchain/mod.rs` is now only handler registration and
module wiring.
RPC transport lifecycle follows the same split:
`rpc_server/lifecycle.rs` owns jsonrpsee startup, shutdown, TLS placeholder
handling, DoS-limit builder wiring, and session-purge task wiring;
`rpc_server/handler.rs` owns callback/descriptor bindings, and
`rpc_server/metrics.rs` owns Prometheus request/error counters.
`rpc_server/rate_limit.rs` owns the RPC-server adapter from `RpcServerConfig`
to the governor limiter plus blocked-call error mapping.
RPC TLS follows the same settings/identity/trust split:
`rpc_tls/config.rs` owns `RpcServerConfig` to `rustls::ServerConfig`
orchestration, `rpc_tls/certificate.rs` owns PKCS#12 server identity loading,
and `rpc_tls/authorities.rs` owns trusted client-authority thumbprint
normalization plus native-root filtering. The root `rpc_tls/mod.rs` is now only
facade exports and the module map.
`rpc_server/registry.rs` owns handler registration, handler-map reads, and
transport method projection. The root `rpc_server/mod.rs` now stays focused on
structural server state, while `rpc_server/state.rs` owns construction,
settings, upstream-ledger, WebSocket, and auth-state accessors.
`rpc_server/wallet.rs` owns the active-wallet handle, wallet accessors, and
wallet-change callbacks, and
`rpc_server/sessions.rs` owns invoke-session storage, expiration, mutation,
and termination helpers.
`jsonrpsee_adapter/auth.rs` owns the transport-auth extension marker and Basic
header verification, `jsonrpsee_adapter/codec.rs` owns transport parameter
decoding and Neo error-object projection, `jsonrpsee_adapter/context.rs` owns
the callback context, `jsonrpsee_adapter/module.rs` owns dynamic method
registration and request/error counters, and `jsonrpsee_adapter/dispatch.rs`
owns per-request handler resolution plus auth-gated dispatch. The root adapter
module now keeps only facade exports and the module map.
RPC dispatch has the same production/test boundary now:
`dispatch/mod.rs` owns handler resolution, rate-limit checks, and remote-ledger
proxy dispatch, while `dispatch/panic_policy.rs` owns local handler panic
capture and `UnhandledExceptionPolicy` application. Remote-ledger policy
coverage tests live in `tests/server/core/dispatch.rs`.
`rpc_remote_ledger/policy.rs` owns the remote-ledger proxy method catalog,
`rpc_remote_ledger/client.rs` owns the stable client facade, and
`rpc_remote_ledger/transport.rs` owns blocking upstream RPC calls, shared HTTP
client construction, and response validation. The root remote-ledger module now
keeps only facade exports plus the module map.
RPC relay follows the same split: `rpc_relay/transaction.rs` owns mempool
admission through the blockchain service, `rpc_relay/block.rs` owns block
preflight and import orchestration, `rpc_relay/result.rs` owns relay-result
projection, and `rpc_relay/runtime.rs` owns the synchronous bridge for async
service calls. The root relay module now keeps facade exports and the module
map only.
RPC invocation sessions now follow the same facade rule:
`session/iterators.rs` owns retained iterator registration payloads, traversal
adapters, and disposal; `session/dummy_block.rs` owns the C#-compatible
`ApplicationEngine.CreateDummyBlock` construction used by stateless invokes;
and `session/execution.rs` owns `Session::new`, including the transaction
container, dummy persisting block, native provider threading, and initial script
execution. The root `session/mod.rs` stays focused on retained session state,
diagnostics, snapshots, expiration, and stable iterator IDs.
RPC diagnostics also follow the adapter/state split:
`diagnostic/invocation_tree.rs` owns diagnostic invocation-tree capture,
parent/child traversal, and snapshots, while `diagnostic/mod.rs` owns the
`neo_execution::diagnostic::Diagnostic` trait adapter and public diagnostic
facade.
Indexer block reads have started the same endpoint-family split:
`rpc_server_indexer/blocks.rs` owns `getblockindex` and `getblockindexes`
lookup, while `rpc_server_indexer/params.rs` owns the typed block-selector and
page request records and `rpc_server_indexer/responses.rs` owns optional/list
block-index projection. `getindexerstatus` uses the shared no-parameter request
record through `params.rs`; `status.rs` owns status handling and status lookup,
`support.rs` owns IndexerService lookup, shared error mapping, page bounds, and
block selector types, while `responses.rs` owns the status and ApplicationLogs
availability projection. Indexer endpoints no longer carry private duplicate
validators or inline status envelopes.
`rpc_server_indexer/transactions.rs` owns transaction lookup and
block/address/contract transaction list routing, while `params.rs` owns the
typed transaction-hash and block-page request records for transaction index
lookups and block transaction pages, plus address-page and contract-activity
records for account and contract transaction queries. `responses.rs` owns
transaction, account-transaction, optional-result, and list projection.
`rpc_server_indexer/notifications.rs` owns address/block/transaction/contract
notification routing, while `params.rs` owns the shared account, block,
transaction, and contract-activity page request records and `responses.rs` owns
notification list projection. The root `rpc_server_indexer/mod.rs` now keeps
only handler registration.
RPC settings parsing has started the same decomposition:
`rpc_server_settings/gas.rs` owns C#-compatible `MaxGasInvoke` and `MaxFee`
GAS/datoshi decoding; `rpc_server_settings/config.rs` owns
the `RpcServerConfig` serde schema, C# aliases/defaults, duration projections,
and redacted debug formatting; and `rpc_server_settings/registry.rs` owns
process-wide loading, validation, and lookup. The root
`rpc_server_settings/mod.rs` now keeps only the module map, exception-policy
enum, and public re-exports.
RPC errors now follow the same catalog split: `rpc_error/catalog.rs` owns the
C#-compatible named error constructors and contract-verification formatter,
`rpc_error/record.rs` owns the `RpcError` record, data trimming, JSON
projection, and `Display` / `Error` implementations, and `rpc_error/mod.rs`
keeps only facade exports and the module map.
Shared RPC helpers now follow the same facade rule:
`rpc_helpers/errors.rs` owns common `RpcException` constructors,
`params.rs` owns generic positional parsing and no-parameter request
validation, `bytes.rs` owns Base64 and Neo wire-payload helpers, and
`hashes.rs` owns address/UInt160/UInt256 parsing.
The root `rpc_helpers/mod.rs` keeps the public helper API as re-exports plus
the module map.
Smart-contract request parsing now follows the same rule:
`smart_contract/request.rs` owns positional parsing for invocation, contract
verification, iterator-session, and unclaimed-GAS handlers, including
signer/witness conversion, diagnostic defaults, UUID/count decoding, and
address/hash normalization. The handler files stay focused on contract lookup,
VM execution, iterator sessions, and native GAS queries,
while `smart_contract/invocation_wallet.rs` owns wallet transaction
materialization, available-account signing, and pending-signature projection for
successful invokes. `smart_contract/diagnostics.rs` owns invoke diagnostic-tree
and storage-change JSON projection. `smart_contract/script.rs` owns dynamic-call
script construction and contract-parameter stack conversion.
`smart_contract/response.rs` owns invoke/contract-verification result
envelopes, VM-state, stack-item, iterator-interface, iterator-session,
notification, and unclaimed-GAS JSON projection.
Production `#[allow(...)]` sites now carry nearby `Rationale:` comments. The
comments classify each exception as protocol parity, generated NeoFS glue, HSM
or BLST FFI shape, VM unsafe hot-path invariants, explicit composition arity,
or client/RPC compatibility facade behavior. Test-only allows remain outside
this production-rationale rule.
`neo-hsm/src/settings/config.rs` owns operator-facing HSM provider settings.
AWS CloudHSM environment loading now returns typed `HsmError::Init` values for
missing `NEO_HSM_CU_PASSWORD` instead of panicking before consensus startup.
`neo-config/src/settings/protocol/presets.rs` now exposes fallible
`try_mainnet` / `try_testnet` constructors for embedded committee diagnostics.
The existing infallible preset constructors stay API-compatible and are guarded
by tests that assert the built-in committee literals parse to 21 public keys.
`neo-node/src/node/config.rs` uses those fallible constructors while deriving
daemon protocol settings so malformed embedded presets become startup errors.
`neo-runtime/src/service/sync_pipeline.rs` decodes staged-sync checkpoint
payloads through checked fixed-width readers instead of slice-length `expect`
calls, preserving checkpoint bytes while keeping corrupted store data typed.
`neo-runtime/src/service/service_registry.rs` now handles erased-service
downcast mismatches by returning `None`; the TypeId invariant remains internal,
but a registry mismatch no longer aborts an RPC or node service lookup path.
`neo-primitives/src/payload/serializable_payload.rs` now materializes the
SHA-256 payload digest into a fixed `[u8; 32]` before constructing `UInt256`.
This removes the production `expect()` path while preserving Neo N3's
single-SHA payload hash semantics.
Generated fixed-width uint types now expose `from_array` for callers that
already hold `[u8; N]`, keeping array construction infallible while preserving
typed errors for variable-length slice decoding. `UInt160::from_script` and
native registry hash construction use that path instead of fallback-to-zero
conversion.
`BigDecimal` stores its scale as `u32` internally, so multiplying two values
created from valid `u8` token decimals no longer routes through a production
`expect()` on decimal overflow. The direct `neo-primitives` production scan now
leaves only doc-comment unwrap examples in that crate; the aggregate audit
bucket still reports those examples until the scanner distinguishes doctests
from production code.
`neo-native-contracts/src/neo_token/storage/candidates.rs` keeps committee
top-list pruning panic-free by checking the current worst candidate explicitly
instead of asserting the full-list invariant through `expect`.
`neo-native-contracts/src/registry/hashes.rs` stores canonical native contract
hashes as fixed little-endian byte arrays, avoiding runtime hex parsing and
`expect` in native contract registry initialization.
`neo-storage` RocksDB/MDBX store and snapshot read paths now log backend read
errors and return `None` consistently in all build modes instead of panicking
under debug assertions. This keeps storage API behavior aligned with `Option`
returns and avoids `panic=abort` process exits on read-error paths.
`neo-storage/src/types/storage_item.rs` routes `to_value()` through the existing
`value_bytes()` materialization path, removing a duplicated cache invariant
`expect` while keeping raw-byte and cache-backed value semantics unchanged.
`neo-storage/src/persistence/traits/store_factory.rs` now follows the crate's
external-test layout by pointing at `src/tests/persistence/store_factory.rs`,
so provider-factory assertions no longer look like production panic surfaces.

Recommended next patches, in order:

1. Apply the typed request/response-helper pattern to the remaining `neo-rpc`
   handler groups, using the wallet request split as the local template.
