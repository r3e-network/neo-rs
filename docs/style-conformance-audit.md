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
- `neo-config/src/settings/protocol.rs` is now a typed settings facade. Built-in
  network presets live in `settings/protocol/presets.rs`, file/stream loading
  lives in `settings/protocol/load.rs`, JSON/raw config parsing lives in
  `settings/protocol/parse.rs`, and hardfork sequence rules live in
  `settings/protocol/validation.rs`.
- `neo-node/src/node/chain_acc/mod.rs` and `neo-node/src/node/fast_sync/mod.rs`
  are very large workflow modules. They should be split into domain files such
  as `format`, `reader`, `import`, `report`, `package`, `manifest`, and
  `workflow` while keeping behavior locked by tests.
- `neo-node/src/node/mod.rs` still mixes CLI validation, storage preflight,
  fast-sync mode cleanup, P2P wiring, inventory handling, RPC startup, and
  shutdown. It needs a facade-oriented startup workflow, but only after focused
  regression tests protect the modes.
- `neo-node/src/node/logging/mod.rs` is now a facade for logging setup:
  `logging/filter.rs` owns `RUST_LOG` / TOML directive selection,
  `logging/format.rs` owns operator-facing format parsing, and
  `logging/rotation.rs` owns file writer construction plus size-based archive
  rotation.
- `neo-node/src/node/services/mod.rs` now focuses on operational service
  composition. `services/state.rs` owns StateService MPT store and
  sync/async commit-handler construction, `services/read_side.rs` owns
  indexer, ApplicationLogs, and TokensTracker construction, while
  `services/store.rs` owns service-store opening, storage config inheritance,
  and fast-sync backend mode.
- `neo-rpc` has the largest raw JSON surface. Many `Value` uses are correct at
  the transport edge, but handler internals should move repeated request and
  response shapes into typed parameter/result modules.
- `neo-execution`, `neo-vm`, and `neo-native-contracts` contain legitimate C#
  parity and interop boundaries, but production `expect` sites and broad
  `dyn Any` escape hatches should be reviewed one by one and either converted
  to typed errors/contexts or documented as impossible invariants.
- `neo-execution/src/lib.rs` still re-exports internal implementation modules
  broadly. Keep crate-root exports to deliberate facade/domain types after
  downstream imports are mapped.
- `neo-native-contracts/src/lib.rs` contains NEP-17 stack-item construction,
  payment decoding, storage-byte helpers, and method builders. Move these into
  owned domain modules so the root remains a map.
- `neo-indexer/src/indexer/mod.rs` is now closer to a mutable projection
  facade: `indexer/block.rs` owns canonical block and transaction
  materialization before records are applied to the in-memory indexes.
- `neo-indexer/src/service/mod.rs` keeps the service facade and mutation
  orchestration, while `service/backend.rs` owns durable backend kind,
  diagnostic paths, mutation mode selection, and persistence dispatch.
- `neo-storage` exposes broad `dyn Store` / `dyn StoreSnapshot` boundaries.
  This is valid for backend selection, but hot loops should keep borrowed
  visitor APIs or concrete paths where possible.
- `neo-state-service` has a few public erased compatibility surfaces such as
  `Verifier<C = Arc<dyn StateRootCalculator>>` and
  `StateStore::with_mpt_store(..., Arc<dyn Store>)`. Prefer concrete
  constructors as the primary surface and name erased constructors explicitly.
- `neo-gui` is outside the workspace and has several `Mutex::lock().unwrap()`
  sites. It should either handle poison errors at the UI boundary or centralize
  locking helpers.
- `neo-oracle-service` has the densest lint-allow count and several
  generated/NeoFS adapter modules. Allows should be narrowed or annotated.
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
state-root / `findstates` JSON construction lives in
`rpc_server_state/response.rs`. State proof handlers and the C# proof-payload
codec now live in `rpc_server_state/proof.rs`. Historical trie lookup mechanics
for `getstate` / `findstates`, including root gating, storage-key construction,
and C#-compatible trie error mapping, now live in
`rpc_server_state/state_queries.rs`. The handler module now owns only handler
registration, StateStore/MPT service lookup, and state-root metadata responses.
Parameter conversion now follows the same module-map rule:
`parameter_converter/scalar.rs` owns string, boolean, numeric, Base64 bytes,
and UUID `RpcConvertible` implementations. `parameter_converter/domain.rs`
owns address arrays, block hash/index identifiers, and contract
name/hash/id conversions. `parameter_converter/contract_parameters.rs` owns
contract-parameter array conversion. `parameter_converter/tokens.rs` owns
generic `JToken` shape checks, numeric coercion, and serde JSON projection.
The root `parameter_converter/mod.rs` keeps the converter facade, context,
trait, and domain identifier helpers.
The same split now covers token tracker handlers:
`rpc_server_tokens_tracker/request.rs` owns account/time-window/token-id
parsing, while `rpc_server_tokens_tracker/response.rs` owns common balance and
transfer response envelopes. Handler mechanics are now separated by endpoint
family as well: `balances.rs` owns NEP-11/NEP-17 balance enrichment,
`transfers.rs` owns transfer-history routing, and `properties.rs` owns the
NEP-11 property VM invocation. The root token-tracker module is now just the
method registry plus module map.
Wallet cleanup has started with the same boundary: `rpc_server_wallet/request.rs`
now owns management and network-fee request decoding (`dumpprivkey`,
`getwalletbalance`, `importprivkey`, `openwallet`, `calculatenetworkfee`), so
the wallet root handler can focus on wallet orchestration, native balance
queries, and fee calculation. The same request module now also owns transfer,
`sendmany`, signer, and cancel-transaction parameter decoding, leaving
`transfers.rs` focused on descriptor lookup, amount conversion, transaction
construction, and transfer/cancel orchestration. `rpc_server_wallet/signing.rs`
owns C# `Wallet.Sign` parity, witness completion, network-fee adjustment, and
relay result projection. `rpc_server_wallet/lifecycle.rs` owns open/close,
address creation/listing, and WIF import/export endpoints; the root keeps shared
wallet runtime helpers used across lifecycle and transfer paths, while
`rpc_server_wallet/errors.rs` owns wallet-domain error projection and transfer
insufficient-funds compatibility mapping into RPC exceptions.
`rpc_server_wallet/balance.rs` owns `getwalletbalance`,
`getwalletunclaimedgas`, and the native balance/unclaimed-GAS probe logic.
`rpc_server_wallet/network_fee.rs` owns `calculatenetworkfee` request execution
and wallet-account script projection.
ApplicationLogs now follows the same split: `rpc_server_application_logs/request.rs`
owns hash and trigger-filter parsing, and `response.rs` owns the optional
trigger filtering over stored C#-compatible log JSON. Direct handler tests cover
transaction-log lookup, trigger filtering, unknown hashes, and missing service
errors.
Oracle submission follows the request-boundary rule as well:
`rpc_server_oracle/request.rs` owns Base64 decoding, request-id parsing, and
secp256r1 public-key validation for `submitoracleresponse`; the handler keeps
only service lookup, submission, and `OracleServiceError` mapping.
Utility endpoints now use the same pattern: `rpc_server_utilities/request.rs`
owns no-parameter validation for `listplugins` / `listservices` and
`validateaddress` parameter parsing, while the root handler stays focused on
inventory lookup and address validation.
Node relay methods now follow the same boundary:
`rpc_server_node/request.rs` owns Base64 decoding and Neo wire-payload
deserialization for `sendrawtransaction` and `submitblock`;
`rpc_server_node/relay.rs` owns endpoint orchestration, while
`rpc_relay/result.rs` owns C#-compatible relay-result mapping for both node and
wallet submission paths, and `rpc_relay/runtime.rs` owns the synchronous bridge
into async blockchain service calls.
Node version reporting now follows the same endpoint-family split:
`rpc_server_node/version.rs` owns C#-compatible `getversion` response
construction, dynamic Policy storage readers, remote-ledger version projection,
and hardfork/public-key formatting. `rpc_server_node/status.rs` owns
`getconnectioncount`, `getpeers`, and shared local-node projection for node
status/version handlers. The root `rpc_server_node/mod.rs` is now only the
handler-registration facade and module map.
Blockchain storage methods now follow that request-boundary pattern:
`rpc_server_blockchain/request_helpers.rs` owns contract identifier and Base64
key/prefix/start parsing for `getstorage` and `findstorage`, while
`storage.rs` keeps contract resolution, storage lookup, pagination, and
response construction.
Blockchain mempool handling has been moved out of the large route map:
`rpc_server_blockchain/mempool.rs` owns `getrawmempool` live-pool reads and
response construction, while `request_helpers.rs` owns `shouldGetUnverified`
parsing and `mod.rs` stays closer to registration plus remaining legacy groups.
Blockchain transaction lookup now follows the same split:
`rpc_server_blockchain/transactions.rs` owns `getrawtransaction` and
`gettransactionheight` pool/ledger lookup plus C#-compatible verbose
projection, while `request_helpers.rs` owns hash and verbose parsing.
Blockchain native/governance queries have moved out of the route map:
`rpc_server_blockchain/native.rs` owns native contract listing, committee,
validator, and candidate projections over `NativeQueries`.
`native_queries/script.rs` owns the C# `EmitDynamicCall` bytecode layout for
read-only native probes; `native_queries/result.rs` owns NEO stack-result
decoding for committee, validator, and candidate probes;
`native_queries/execution.rs` owns read-only VM setup and HALT validation;
`native_queries/registry.rs` owns standard native-contract registry
construction; and `native_queries/mod.rs` keeps the public query facade.
Blockchain block/header methods now follow the same route-facade split:
`rpc_server_blockchain/blocks.rs` owns best hash, block/header counts,
block-hash lookup, block/header retrieval, and block system-fee calculation.
The root `rpc_server_blockchain/mod.rs` is now only handler registration and
module wiring.
RPC transport lifecycle follows the same split:
`rpc_server/lifecycle.rs` owns jsonrpsee startup, shutdown, TLS placeholder
handling, DoS-limit builder wiring, and session-purge task wiring;
`rpc_server/handler.rs` owns callback/descriptor bindings, and
`rpc_server/metrics.rs` owns Prometheus request/error counters.
`rpc_server/rate_limit.rs` owns the RPC-server adapter from `RpcServerConfig`
to the governor limiter plus blocked-call error mapping.
`rpc_server/registry.rs` owns handler registration, handler-map reads, and
transport method projection. The root `rpc_server/mod.rs` now stays focused on
structural server state, while `rpc_server/wallet.rs` owns the active-wallet
handle, wallet accessors, and wallet-change callbacks, and
`rpc_server/sessions.rs` owns invoke-session storage, expiration, mutation,
and termination helpers.
`jsonrpsee_adapter/auth.rs` owns the transport-auth extension marker and Basic
header verification, `jsonrpsee_adapter/codec.rs` owns transport parameter
decoding and Neo error-object projection, and `jsonrpsee_adapter/mod.rs` keeps
module registration and dispatch bridging.
RPC dispatch has the same production/test boundary now:
`dispatch/mod.rs` owns handler resolution, rate-limit checks, and remote-ledger
proxy dispatch, while `dispatch/panic_policy.rs` owns local handler panic
capture and `UnhandledExceptionPolicy` application. Remote-ledger policy
coverage tests live in `tests/server/core/dispatch.rs`.
`rpc_remote_ledger/policy.rs` owns the remote-ledger proxy method catalog,
while `rpc_remote_ledger/mod.rs` keeps the blocking upstream RPC client and
response validation.
RPC invocation sessions now follow the same facade rule:
`session/iterators.rs` owns retained iterator registration payloads, traversal
adapters, and disposal; `session/dummy_block.rs` owns the C#-compatible
`ApplicationEngine.CreateDummyBlock` construction used by stateless invokes;
and `session/execution.rs` owns `Session::new`, including the transaction
container, dummy persisting block, native provider threading, and initial script
execution. The root `session/mod.rs` stays focused on retained session state,
diagnostics, snapshots, expiration, and stable iterator IDs.
Indexer block reads have started the same endpoint-family split:
`rpc_server_indexer/blocks.rs` owns `getblockindex` and `getblockindexes`
lookup, pagination, and block-index projection.
`rpc_server_indexer/transactions.rs` owns transaction lookup and
block/address/contract transaction list routing.
`rpc_server_indexer/notifications.rs` owns address/block/transaction/contract
notification routing. The root `rpc_server_indexer/mod.rs` now keeps handler
registration, service lookup, shared error mapping, and shared selector types.
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
while `rpc_error/mod.rs` owns the `RpcError` record, data trimming, JSON
projection, and `Display` / `Error` implementations.
Shared RPC helpers now follow the same facade rule:
`rpc_helpers/errors.rs` owns common `RpcException` constructors,
`params.rs` owns generic positional parsing, `bytes.rs` owns Base64 and Neo
wire-payload helpers, and `hashes.rs` owns address/UInt160/UInt256 parsing.
The root `rpc_helpers/mod.rs` keeps the public helper API as re-exports plus
the module map.
Smart-contract request parsing now follows the same rule:
`smart_contract/request.rs` owns positional parsing for invocation, contract
verification, iterator-session, and unclaimed-GAS handlers, including
signer/witness conversion, diagnostic defaults, UUID/count decoding, and
address/hash normalization. The handler files stay focused on contract lookup,
VM execution, iterator sessions, native GAS queries, and result projection,
while `smart_contract/invocation_wallet.rs` owns wallet transaction
materialization, available-account signing, and pending-signature projection for
successful invokes.

Recommended next patches, in order:

1. Apply the typed request/response-helper pattern to the remaining `neo-rpc`
   handler groups, using the wallet request split as the local template.
2. Centralize GUI lock handling in `neo-gui` before fixing individual
   `lock().unwrap()` call sites.
3. Add comments to every remaining production `#[allow]` that explain the
   protocol, FFI, generated-code, or C# parity reason.
