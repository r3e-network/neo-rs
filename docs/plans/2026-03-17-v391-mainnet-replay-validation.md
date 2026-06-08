# Neo v3.9.1 Mainnet Replay Validation

## Confirmed fix

The replay blocker around blocks `38781 -> 38791 -> 38883` was resolved by fixing
memory-backend backward prefix scans:

- [memory_store.rs](/home/neo/git/neo-rs/neo-core/src/persistence/providers/memory_store.rs)
- [memory_snapshot.rs](/home/neo/git/neo-rs/neo-core/src/persistence/providers/memory_snapshot.rs)

This affected native NEO bonus lookup via backward prefix iteration over
`gasPerBlock` records on memory-backed replay.

## Rebuilt node requirement

An intermediate validation run was misleading because the live replay process was
still using an older `target/debug/neo-node` binary. After rebuilding
`neo-node`, replay from genesis passed the old failure height.

## Code verification

The following passed after the fix and expectation updates:

- `cargo test -p neo-core -- --test-threads=1`
- `cargo test -p neo-core --features runtime --test tokens_tracker_nep17_csharp_parity -- --exact --nocapture`
- `cargo test -p neo-rpc --features server -- --test-threads=1`
- `cargo build -p neo-node`

## Mainnet replay validation

Local replay RPC:

- `http://127.0.0.1:65332`

Confirmed on rebuilt replay:

- local chain advanced past block `38883`
- checkpoint tx parity:
  - `0x8f7ac7f1740fb93407a9a2ee538c293bf8fe1594d8dbd05e484122a6f377a99e`
  - `0x462eda0bd9727efd90b0cf09d74a5023826610964db1e384b2e48962a8f948a7`
  - `0xc68aac4b0bb9e88bd42086c50cebe648ad28726d2849ff73faeb93985e510587`
  - `0x6c12841f2477e13b375ef22ec9bfcc5288ed68b0d1b5fc97d4c6c3a7bcf7b90d`
  - `0x21b17473c89da950f34ff38dc6a305a0ec3c054974797ed722edfa59bf5643be`
  - `0x713b87027b621bd951feb36c3d3727798e70089b5868a6a8432bc80e7569e5ad`

## Blockhash parity windows

Contiguous parity confirmed:

- `14480..14520`
- `21280..21390`
- `38770..38920`
- `52950..53150` pending/running during extended validation

Sampled parity confirmed:

- sampled blockhashes through `39200`
- sampled blockhashes near local tip around `55100`

## Transaction-level parity samples

Full tx parity confirmed:

- `38770..38920`

Sampled non-empty block tx parity confirmed across:

- `0..50000` in `5k` buckets
- `50000..82680` in `5k` buckets
- post-blocker sample `38921..38940`
- repaired-window sample around `21288/21373`
- repaired-window sample around `14492`

## Tooling added

- [check-v391-mainnet-checkpoints.py](/home/neo/git/neo-rs/scripts/check-v391-mainnet-checkpoints.py)
  - local/public checkpoint verifier
  - degrades gracefully when local RPC lacks `getapplicationlog`
- [build-acc-from-rpc.py](/home/neo/git/neo-rs/scripts/build-acc-from-rpc.py)
  - builds `.acc` payloads from RPC `getblock(height, 0)` responses
- [compare-local-csharp-rust-sync-samples.py](/home/neo/git/neo-rs/scripts/compare-local-csharp-rust-sync-samples.py)
  - now accepts `--candidate-heights` for wider replay sampling

## Additional RPC fixes validated

- [rpc_server_blockchain.rs](/home/neo/git/neo-rs/neo-rpc/src/server/rpc_server_blockchain.rs)
  - confirmed `getrawtransaction(txid, true)` now includes `vmstate` for persisted transactions
- [extensions.rs](/home/neo/git/neo-rs/neo-core/src/tokens_tracker/extensions.rs)
  - fixed `find_range()` misuse of prefix iteration so NEP-11/NEP-17 transfer history range queries work correctly
- [nep17_tracker.rs](/home/neo/git/neo-rs/neo-core/src/tokens_tracker/trackers/nep_17/nep17_tracker.rs)
  - reverted an incorrect fresh-import change and aligned history recording with the upstream C# TokensTracker semantics
  - history is recorded only for transaction-backed `Transfer` notifications
  - `transfernotifyindex` is a single monotonically increasing per-tracker index, not a per-user/per-direction index
- [nep11_tracker.rs](/home/neo/git/neo-rs/neo-core/src/tokens_tracker/trackers/nep_11/nep11_tracker.rs)
  - aligned the NEP-11 history logic to the same C# tracker rules
- [rpc_blockchain_getrawtransaction_vmstate.rs](/home/neo/git/neo-rs/neo-rpc/tests/rpc_blockchain_getrawtransaction_vmstate.rs)
  - regression coverage for `getrawtransaction(..., true).vmstate`
- [tokens_tracker_nep17_csharp_parity.rs](/home/neo/git/neo-rs/neo-core/tests/tokens_tracker_nep17_csharp_parity.rs)
  - regression coverage for the C# `getnep17transfers` history indexing semantics under the runtime feature
- [rpc_helpers.rs](/home/neo/git/neo-rs/neo-rpc/src/server/rpc_helpers.rs)
  - normalized server-side address and `UInt256` parse failures to the C# RPC wording
- [rpc_server_tokens_tracker.rs](/home/neo/git/neo-rs/neo-rpc/src/server/rpc_server_tokens_tracker.rs)
  - tracker RPC invalid-address errors now use `Invalid Address: ...`
  - `endTime < startTime` now matches the C# `Invalid params` shape without an extra `data` field
- [smart_contract/unclaimed_gas.rs](/home/neo/git/neo-rs/neo-rpc/src/server/smart_contract/unclaimed_gas.rs)
  - `getunclaimedgas` now normalizes invalid addresses to the C# RPC error shape
- [rpc_server_application_logs.rs](/home/neo/git/neo-rs/neo-rpc/src/server/rpc_server_application_logs.rs)
  - `getapplicationlog` now matches C# for:
    - invalid `UInt256` input wording
    - missing/null `hash` parameter shape
    - unknown transaction/blockhash response shape
    - numeric or non-string trigger filters
- [blockchain_application_executed.rs](/home/neo/git/neo-rs/neo-core/src/ledger/blockchain_application_executed.rs)
  - normalized native block-phase `BREAK` states to `HALT` for persisted execution records, matching C# block `ApplicationLogs`

## Tracker import validation

Fresh tracker-enabled `.acc` imports were revalidated against Neo C# `3.9.1`
after rebuilding `neo-node`.

Reference RPC:

- `http://seed1.neo.org:10332`

Fresh imported tracker RPC:

- `http://127.0.0.1:61362`

Confirmed on a clean import of `/tmp/neo-mainnet-0-39000-local.acc`:

- `getnep17transfers` matches C# on the original repaired windows for:
  - `NS7Ta2zM9mmpTR6LvoabM86yZmP8sxxHAo`
  - `NZuaP5GJ1vuk979qLiFJSHgFD4Jiuii8GB`
- the fresh import no longer records extra `OnPersist` burn history rows
- `transfernotifyindex` now matches the C# transaction-wide indexing semantics
- a wider NEP-17 neighborhood sample matched across 12 related addresses in the
  `1628510000000..1628512800000` window
- sampled `getnep11transfers` queries for the same repaired window matched the
  C# node on the tested addresses

An earlier hypothesis that C# recorded additional block-burn history rows or
used per-direction indexes was disproven by direct comparison against the live
`/Neo:3.9.1/` RPC and by the upstream `neo-modules` tracker source.

## Additional live RPC parity

Reference RPC:

- `http://seed1.neo.org:10332`

Imported tracker RPC:

- `http://127.0.0.1:61362`

Imported ApplicationLogs RPC:

- `http://127.0.0.1:61382`
- `http://127.0.0.1:61402`

Confirmed in this pass:

- tracker/address RPC parity on the imported node for:
  - `getnep17transfers`
  - `getnep11transfers`
  - `getnep17balances`
  - `getnep11balances`
  - `getnep11properties`
  - `getunclaimedgas`
- invalid-address error wording now matches C# on those methods
- tracker `endTime < startTime` errors now match the C# shape without `data`
- `getapplicationlog(txid)` parity holds on sampled historical transactions:
  - `0x8f7ac7f1740fb93407a9a2ee538c293bf8fe1594d8dbd05e484122a6f377a99e`
  - `0x462eda0bd9727efd90b0cf09d74a5023826610964db1e384b2e48962a8f948a7`
  - `0xc68aac4b0bb9e88bd42086c50cebe648ad28726d2849ff73faeb93985e510587`
  - `0x6c12841f2477e13b375ef22ec9bfcc5288ed68b0d1b5fc97d4c6c3a7bcf7b90d`
  - `0x21b17473c89da950f34ff38dc6a305a0ec3c054974797ed722edfa59bf5643be`
  - `0x713b87027b621bd951feb36c3d3727798e70089b5868a6a8432bc80e7569e5ad`
- `getapplicationlog(blockhash)` parity now holds on the repaired block:
  - `0x9a4120415e90b4b0aa5c2b74ff22526840547a77eb25f0f7ef4ba80ccd4bd98f`
- `getapplicationlog` now matches C# for:
  - missing `hash`
  - null `hash`
  - invalid `UInt256`
  - unknown transaction/blockhash
  - numeric trigger filters
- `getrawtransaction` / `gettransactionheight` invalid-hash wording now matches C#

## StateService parity fix

The remaining `StateService` divergence was traced to `BinarySerializer`
rebuilding `StackItem::Map` values through a sorted `BTreeMap` during
deserialization. That preserved map semantics but changed serialized byte order,
which in turn changed contract storage bytes and later state roots.

Fixed in:

- [binary_serializer.rs](/home/neo/git/neo-rs/neo-core/src/smart_contract/binary_serializer.rs)
  - `StackItemType::Map` now preserves serialized insertion order by rebuilding
    via `OrderedDictionary`
- [committee.rs](/home/neo/git/neo-rs/neo-core/src/smart_contract/native/neo_token/committee.rs)
  - updated the committee-cache genesis fixture to the C#-compatible empty
    integer encoding
- [native_contract_tests.rs](/home/neo/git/neo-rs/neo-core/tests/native_contract_tests.rs)
  - updated the CryptoLib genesis fixture to the on-chain `curve` manifest
    parameter name; RPC still remaps to `curveHash`

Regression coverage:

- `smart_contract::binary_serializer::tests::deserialize_preserves_map_entry_order_for_roundtrip_bytes`

Fresh imported StateService validation after the fix:

- short clean import `0..14510`:
  - `getstateroot(14498)` matched C#
  - `getstateroot(14500)` matched C#
  - `getstateroot(14510)` matched C#
- full clean import `0..39000`:
  - `getstateroot(14498)` matched C#
  - `getstateroot(30000)` matched C#
  - `getstateroot(39000)` matched C#

## Remaining caution

This is strong evidence for the repaired historical regions, but it is still not
a formal proof of global `100%` compatibility across all heights, methods, and
runtime states.
