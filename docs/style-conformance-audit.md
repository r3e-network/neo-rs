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
`rpc_server_state/response.rs`. The handler module now orchestrates services,
tries, and proof generation instead of owning JSON parameter layout details.
The same split now covers token tracker handlers:
`rpc_server_tokens_tracker/request.rs` owns account/time-window/token-id
parsing, while `rpc_server_tokens_tracker/response.rs` owns common balance and
transfer response envelopes.
Wallet cleanup has started with the same boundary: `rpc_server_wallet/request.rs`
now owns management and network-fee request decoding (`dumpprivkey`,
`getwalletbalance`, `importprivkey`, `openwallet`, `calculatenetworkfee`), so
the wallet root handler can focus on wallet orchestration, native balance
queries, and fee calculation. The same request module now also owns transfer,
`sendmany`, signer, and cancel-transaction parameter decoding, leaving
`transfers.rs` focused on descriptor lookup, amount conversion, transaction
construction, signing, and relay.
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
deserialization for `sendrawtransaction` and `submitblock`; the root handler
keeps relay submission and relay-result mapping.
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
validator, and candidate projections over `NativeQueries`; `mod.rs` now keeps
only registration plus the remaining block/header legacy handlers.
RPC transport lifecycle follows the same split:
`rpc_server/lifecycle.rs` owns jsonrpsee startup, shutdown, TLS placeholder
handling, DoS-limit builder wiring, and session-purge task wiring; the root
`rpc_server/mod.rs` now stays focused on server state, handler registration,
wallet/session accessors, and rate-limit policy.
RPC settings parsing has started the same decomposition:
`rpc_server_settings/gas.rs` owns C#-compatible `MaxGasInvoke` and `MaxFee`
GAS/datoshi decoding; `rpc_server_settings/config.rs` owns
`RpcServerConfig` default construction and redacted debug formatting; and
`rpc_server_settings/mod.rs` keeps the serde-visible config record,
process-wide registry, and validation.
Smart-contract request parsing now follows the same rule:
`smart_contract/request.rs` owns positional parsing for invocation, contract
verification, iterator-session, and unclaimed-GAS handlers, including
signer/witness conversion, diagnostic defaults, UUID/count decoding, and
address/hash normalization. The handler files stay focused on contract lookup,
VM execution, wallet signing, iterator sessions, native GAS queries, and result
projection.

Recommended next patches, in order:

1. Apply the typed request/response-helper pattern to the remaining `neo-rpc`
   handler groups, using the wallet request split as the local template.
2. Centralize GUI lock handling in `neo-gui` before fixing individual
   `lock().unwrap()` call sites.
3. Add comments to every remaining production `#[allow]` that explain the
   protocol, FFI, generated-code, or C# parity reason.
