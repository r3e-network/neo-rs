# Neo Rust Node – Parity Audit Notes

This report tracks outstanding gaps and follow-up work needed to keep the `neo-rs`
full node aligned with the canonical C# implementation.

## Persistence / Ledger
- ✔ `NeoSystem::persist_block` now mirrors C# snapshot cloning and per-transaction
  execution, but we still rely on a best-effort plugin emission strategy. Confirm
  plugin expectations around `BlockReceived` vs. `TransactionReceived` once more
  parity tests exist.
- ✔ Added TransactionState stack-item roundtrip, clone, and from-replica parity
  coverage (UT_TransactionState).
- ✔ Added LedgerContract `get_block` reconstruction parity coverage from trimmed
  block + transaction states, including header witness checks (UT_TrimmedBlock).
- ✔ Added StorageKey create-with-uint parity coverage (u32/u64 big-endian) to
  match UT_StorageKey overloads.
- ✔ Added StorageKey/KeyBuilder ECPoint parity coverage using a secp256r1 point
  to mirror UT_StorageKey ISerializable overload behavior.
- ✔ Aligned HeaderCache capacity behavior with C# (rejects new headers when full
  instead of evicting) and added parity tests.
- ✔ Added Blockchain relay parity coverage for valid transactions, already-in-pool
  handling, and signer/witness mismatch rejection (UT_Blockchain).
- ✔ Added Blockchain relay parity coverage for on-chain conflict handling with
  same-sender vs cross-sender conflicts (UT_Blockchain malicious conflict case).

## Reverify / Import Pipeline
- ✔ `Blockchain::handle_reverify` now memoizes decoded raw inventories to avoid
  repeated deserialization across reverify requests, aligning with C# caching.
- ✔ Regression tests cover `handle_import` rejection of out-of-order blocks,
  verification failures when `Import.verify == true`, idle scheduling suppression
  when headers are buffered, and raw inventory reverify handling.

## Consensus / dBFT
- ✔ ChangeView threshold logic now counts NewViewNumber >= target view, matching
  DBFTPlugin `CheckExpectedView`.
- ✔ RecoveryRequest handling now limits responses to `f + 1` validators unless
  a commit was already sent, matching DBFTPlugin recovery throttling.
- ✔ RecoveryMessage handling now follows view-based rules (future view change
  payloads only; prepare/commit replay for current view), with recovered
  ChangeView/PrepareRequest/PrepareResponse/Commit payloads reprocessed through
  standard validation (signature + hash checks).
- ✔ Recovery responses now include compact ChangeView/PrepareRequest/PrepareResponse/Commit
  payloads with invocation scripts (primary prepare request included when available),
  matching DBFTPlugin RecoveryMessage composition.
- ✔ Added tests to ensure invalid PrepareRequest/PrepareResponse signatures in
  recovery payloads are ignored.
- ✔ Added recovery tests for invalid commit signatures and mismatched
  PreparationHash handling in recovery PrepareResponses.
- ✔ Added invalid validator index and wrong-view consensus message rejection
  tests for DBFTPlugin parity coverage.
- ✔ Added malformed consensus message decode tests (short buffer, invalid type).
- ✔ Added duplicate PrepareResponse rejection test (same validator resending).
- ✔ Added wrong-block PrepareResponse rejection test for invalid message handling.
- ✔ Added future-block PrepareResponse ignore test (no error, no state change).
- ✔ Added byzantine conflicting PrepareResponse handling coverage (first response
  retained; duplicate rejected).
- ✔ Added backup `on_transactions_received` no-op coverage and timeout-driven
  ChangeView broadcast test for dBFT timer behavior.
- ✔ Added deterministic primary rotation coverage across sequential blocks.
- ✔ Added primary rotation coverage across view changes in the same block.
- ✔ Added PrepareRequest broadcast parity test (primary publishes tx hashes and
  computed hashes).
- ✔ Added multi-round PrepareRequest rotation coverage (primary changes by block).
- ✔ Added PrepareResponse threshold → Commit broadcast parity test.
- ✔ Added commit threshold → BlockCommitted event parity coverage.
- ✔ Added timeout view-change flow coverage leading to a new PrepareRequest.
- ✔ Added view-change integration test that completes consensus on the new view.
- ✔ Added a Rust-native dBFT PersistCompleted integration harness that routes
  consensus broadcasts/events and covers multi-round + committed flows
  (UT_DBFT_Core / UT_DBFT_Integration parity).
- ✔ Wired consensus signing through a pluggable signer that uses wallet/HSM
  accounts; private keys are no longer required for validator mode.
- ✔ Added RPC-driven manual consensus start (startconsensus) for `auto_start=false`.
- ✔ Added mempool NewTransaction filter to cancel transactions with
  `SystemFee > MaxBlockSystemFee`, matching DBFTPlugin.

### Open gaps (node integration)
None.

## Memory Pool
- ✔ Header backlog short-circuit behaviour now matches C#, with a unit test in place.
- ✔ Added PoolItem compare parity coverage (high-priority ordering, fee-based ordering,
  and hash-descending tie-breaker).
- ✔ Added coverage for rebroadcast timing (`BlocksTillRebroadcast` scaling) and
  conflict replacement tracking/fee context.
- ✔ Block persistence now removes conflicting transactions and re-verifies
  unverified entries with the full per-block time budget.
- ✔ Added tests for transaction added/removed events, capacity eviction, try-get
  of unverified entries, and multi-conflict replacement logic.
- ✔ Added test ensuring block-conflict eviction respects shared signer rules.
- ✔ Added tests for sorted transaction ordering, reverify priority, and removing
  unverified entries.
- ✔ Added tests for capacity eviction, header-backlog invalidation, duplicate
  add rejection, conflict chain handling, and invalidate-all behavior.
- ✔ Added tests covering balance-based reverification drops and multi-transaction
  conflict replacement scenarios.
- ✔ Added block-persist conflict tests covering multi-conflict removals and
  shared-signer filtering (mp6/mp7 parity case).
- ✔ Added `can_transaction_fit_in_pool` parity (lowest-fee admission checks) and
  unverified high-priority capacity behavior tests.
- ✔ Added tests covering verified-only transaction vector output and reverify
  limits when verified entries exceed `max_transactions_per_block`.
- ✔ Added iterator parity coverage (verified + unverified enumeration) and
  sorted `verified_and_unverified_transactions` ordering tests.
- ✔ Added conflict-chain parity tests for conflicts referencing non-pooled
  transactions plus sorted-verified subset consistency.
- ✔ Added batch reverification progress coverage to mirror C# multi-batch
  reverify behavior.

## P2P Payloads
- ✔ Added size/validation tests for GetBlocks/GetBlockByIndex payloads.
- ✔ Added HeadersPayload size/roundtrip coverage.
- ✔ Added FilterLoad/FilterAdd payload size/roundtrip + max-K rejection tests.
- ✔ Added InvPayload size, grouping, and invalid-type rejection tests.
- ✔ Added ExtensiblePayload size/roundtrip + witness access checks.
- ✔ Added TransactionAttribute parity tests for HighPriority, NotValidBefore,
  Conflicts, and NotaryAssisted (size/json/serialize/verify/fee) plus
  Transaction GetAttribute retrieval.
- ✔ Added MerkleBlockPayload size + serialization parity tests.
- ✔ Added Signer serialization + JSON parity coverage and fixed CustomGroups
  encoding to match C# (no per-group var-bytes or 0x JSON prefix).
- ✔ Added Witness size/json/max-size parity tests.
- ✔ Added Signer max-nesting And/Or deserialize rejection tests.
- ✔ Added Signer equality semantics coverage.
- ✔ Added Header/Block hex roundtrip fixture tests (UT_Header/UT_Block parity).
- ✔ Added Transaction simple hex roundtrip and invalid signer count/duplicate
  signer deserialization parity tests.
- ✔ Added Transaction JSON parity coverage for sender, signers, and witnesses.
- ✔ Added Transaction JSON parity coverage for null sender when no signers.
- ✔ Added Transaction inventory type and stack-item encoding coverage.
- ✔ Aligned Transaction `from_stack_item` behavior with C# (NotSupportedException).
- ✔ Added Transaction serialize/deserialize hex fixture coverage (UT_Transaction simple hex).
- ✔ Aligned transaction serialization to allow witness/signers mismatch (deserialize rejects), matching C#.
- ✔ Added Transaction reverify hash-length vs witness-length mismatch coverage.
- ✔ Added signature contract fee coverage for multiple allowed contracts (CustomContracts size delta).
- ✔ Added TransactionVerificationContext conflict fee adjustment parity coverage.
- ✔ Added TransactionVerificationContext duplicate oracle response parity coverage.
- ✔ Added TransactionVerificationContext sender fee tracking parity coverage.
- ✔ Updated wallet network fee calculation to include base transaction size (header/signers/attributes/script/witness count),
  fixed multisig invocation sizing, and added Size_Get + CheckNoItems witness verification parity tests.
- ✔ Added signature + multi-sig network fee parity coverage (fee = size gas + verification gas).
- ✔ Added wallet MakeTransaction + ContractParametersContext fee parity coverage for
  `FeeIsSignatureContractDetailed` and `FeeIsMultiSigContract` (size breakdown + verification gas).
- ✔ Fixed wallet transfer script argument encoding (PUSHNULL + PUSHINT) and balanceOf preflight
  stack handling to align MakeTransaction fees with C#.
- ✔ Added signature scope-variant fee parity coverage (Global vs CustomContracts) and VerifyStateIndependent/Dependent
  invalid signature + insufficient funds tests.
- ✔ Added contract-based witness fee calculation support and parity coverage (empty verification script with Verify method).
- ✔ Added VerifyStateDependent hashes/witnesses length mismatch and VerifyStateIndependent multi-sig fixture parity tests.
- ✔ Added Wallet.MakeTransaction scope fault parity tests (CustomContracts, fee-only, missing verification contract) and
  aligned script fee execution with transaction container semantics.
- ✔ Aligned multi-sig redeem script opcodes/parsing with Neo VM, and added ContractParametersContext
  multi-sig ordering + validation parity tests.
- ✔ Adjusted VerifyStateIndependent to match C# handling for non-standard witnesses and added oversize/empty-script
  parity coverage.
- ✔ Added VerifyStateIndependent invalid script and invalid invocation script parity tests.
- ✔ Aligned opcode price table with C# OpCodePriceTable and restored VM pre-execute fee charging so
  signature/multi-sig verification gas parity matches C# (network fee = size gas + verification gas).

## Builders
- ✔ Added TransactionBuilder parity coverage for version/nonce/fees/valid-until,
  script attachment, attributes, witnesses, and signer rules (UT_TransactionBuilder).
- ✔ Aligned SignerBuilder defaults with C# (`WitnessScope::None`) and added
  witness rule/condition builder support.
- ✔ Added TransactionAttributesBuilder parity coverage for conflicts, oracle
  response, high-priority, and not-valid-before attributes (UT_TransactionAttributesBuilder).
- ✔ Aligned TransactionAttributesBuilder duplicate guards for HighPriority and
  NotValidBefore (error messages + per-height uniqueness).
- ✔ Added WitnessBuilder parity coverage for invocation/verification scripts via
  ScriptBuilder and raw byte overloads (UT_WitnessBuilder).
- ✔ Added SignerBuilder parity coverage for account/allowed contracts/allowed
  groups/witness scope/rule handling (UT_SignerBuilder).
- ✔ Added WitnessRuleBuilder support to mirror C# AddWitnessRule + AddCondition flow.
- ✔ Added WitnessConditionBuilder/AndConditionBuilder/OrConditionBuilder parity
  coverage for boolean, group, script hash, called-by, and composite conditions.
- ✔ Added WitnessRuleBuilder parity coverage for script-hash and And-condition rules.

## VM
- ✔ JSON VM test runner now verifies invocation/result stacks to match C# JSON tests.
- ✔ Map stack items now preserve insertion order (OrderedDictionary) for JSON serialization
  and opcode iteration parity with C#.
- ✔ Aligned compound stack item reference semantics (DUP/OVER/PICK) so SETITEM updates
  propagate across shared Array/Struct/Map/Buffer references (C# VM parity).

## Cryptography / Utilities
- ✔ Added Base58 encode/decode vector parity coverage (UT_Base58).

## RPC / Plugin Surface
- ✔ Added NeoFS oracle `neofs:` URL handling via NeoFS REST gateway (payload, range,
  header, and hash responses) plus NeoFS gRPC client parity with C# (signed
  requests, response matryoshka verification, diagnostic JSON formatting,
  bearer-token support, and optional `UseGrpc` override). REST fallback retains
  optional bearer-token headers, base58 ID validation, attribute fallback for
  `X-Attribute-*`, signature/key normalization (hex/base64), and optional bearer
  auto-signing (SHA-512 or wallet-connect) using the active oracle key.
- ✔ Added server-side round-trip tests for `getpeers` and `getrawmempool` to
  validate client model parsing against live handler output.
- ✔ Added `getversion` structure tests to mirror C# RPC expectations.
- ✔ Added utilities RPC tests for `listplugins` (empty response) and `validateaddress`
  invalid checksum/length/empty handling.
- ✔ Added RPC request-processing parity tests for malformed JSON, empty batches,
  mixed batch responses, Basic auth header verification, and protected-method
  auth gating.
- ✔ Added RPC request-processing parity test for sendrawtransaction already-exists
  error code/message in JSON responses.
- ✔ Added sendrawtransaction already-exists test to assert message + empty data payload.
- ✔ Added RpcError parity coverage for unique error strings, wallet fee limit data,
  and AccessDenied JSON shape.
- ✔ Added RpcError wallet fee limit JSON data/message parity check.
- ✔ Added wallet RPC coverage ensuring wallet fee limit errors include data payloads.
- ✔ Added mixed verified/unverified mempool test for `getrawmempool`.
- ✔ Added `sendrawtransaction` invalid payload tests (invalid base64/bytes).
- ✔ Added `sendrawtransaction` null/empty input tests.
- ✔ Added `sendrawtransaction` success-path test with a valid transaction fixture.
- ✔ Added `sendrawtransaction` already-exists test via persisted transaction record.
- ✔ Added `sendrawtransaction` insufficient funds test to mirror C# error mapping.
- ✔ Added `sendrawtransaction` invalid signature test (mutated witness).
- ✔ Added `sendrawtransaction` invalid script/attribute, expired, policy failed, already-in-pool, and oversize coverage.
- ✔ Added `getstorage`/`findstorage` native contract name coverage, `gettransactionheight` mempool-only rejection,
  and invalid contract identifier coverage for `getcontractstate`.
- ✔ Added `findstorage` end-of-pagination empty page parity coverage.
- ✔ Aligned `getnextblockvalidators`/`getcandidates`/`getcommittee` public key formatting to match C# (no `0x` prefix).
- ✔ `getnextblockvalidators`/`getcandidates` now read candidate snapshots (votes + active) to mirror NeoToken behavior.
- ✔ Added `getnextblockvalidators` candidate vote coverage for registered candidates.
- ✔ Added `getcandidates` filtering coverage for blocked/unregistered candidates.
- ✔ Added null-parameter rejection coverage for `getrawtransaction` and `gettransactionheight`.
- ✔ Added `getcandidates` internal error coverage for invalid candidate state data (incl. error data).
- ✔ Added `getrawtransaction` mempool verbose fee field coverage (`sysfee`/`netfee`).
- ✔ Added `getcontractstate` coverage for native contract name/id roundtrips.
- ✔ Added `getnextblockvalidators` coverage for unregistered candidate vote values (-1).
- ✔ Added `getblockheader` verbose confirmation coverage.
- ✔ Added `getrawtransaction` confirmed verbose fee field coverage (`sysfee`/`netfee`).
- ✔ Added `getpeers` coverage for `bad` peer list presence.
- ✔ Added `getversion` hardfork omission coverage for zero-height entries.
- ✔ Aligned `validateaddress` behavior for whitespace inputs (no trimming).
- ✔ Added RPC server settings load parity for C# `RpcServer.json` (PascalCase keys + MaxGasInvoke/MaxFee unit conversion).
- ✔ Added `getnativecontracts` parity coverage for full native contract state list.
- ✔ Added wallet RPC coverage for openwallet + dumpprivkey (including invalid password handling).
- ✔ Added wallet RPC coverage for getnewaddress, getwalletbalance (valid/invalid asset), getwalletunclaimedgas, importprivkey (invalid WIF + existing key), listaddress, and dumpprivkey unknown-account/invalid-address handling.
- ✔ Added openwallet parity coverage for missing file and invalid wallet format.
- ✔ Added openwallet invalid-password data and dumpprivkey unknown-account data parity checks.
- ✔ Added wallet RPC coverage for sendfrom/sendtoaddress/sendmany success paths, no-wallet gating, invalid parameter cases, and sendmany empty-output validation (plus calculatenetworkfee param rejection).
- ✔ Aligned `sendmany` non-positive amount error messaging with C# ("Amount of '{assetId}' can't be negative.").
- ✔ Aligned `sendmany` invalid `to` parameter errors with C# ("Invalid 'to' parameter: ...").
- ✔ Added sendfrom insufficient-funds invalid-request parity coverage.
- ✔ Added sendtoaddress/sendmany insufficient-funds invalid-operation parity coverage.
- ✔ Aligned wallet RPC methods to require an opened wallet before parameter validation (sendfrom/sendtoaddress/sendmany).
- ✔ Added calculatenetworkfee success-path network fee payload coverage.
- ✔ Added wallet RPC canceltransaction coverage (no-wallet, invalid params, empty signers, success-path conflicts attribute output, extraFee parsing, and mempool fee bump).
- ✔ Added closewallet no-open-wallet parity coverage.
- ✔ Added `submitblock` invalid payload tests (invalid base64/bytes).
- ✔ Added `submitblock` success-path test with a valid signed block.
- ✔ Added `submitblock` already-exists test after persisting a block.
- ✔ Added `submitblock` verification-failed tests (empty witness, bad prev hash, bad index).
- ✔ Added `submitblock` null/empty input tests.
- ✔ Added `getbestblockhash` test with explicit current hash state.
- ✔ Added `getblockcount` and `getblockheadercount` defaults coverage.
- ✔ Added `getblockhash` test for stored height.
- ✔ Added `getblock` tests (hash/index roundtrip, verbose confirmations, null param, genesis/no-tx blocks).
- ✔ Added `getblockheader` tests (raw/verbose, null param).
- ✔ Added RPC server `getblocksysfee` handler returning summed system fees with unit coverage.
- ✔ Added RPC parameter conversion tests for numeric ranges, numeric string/whitespace handling,
  safe integer bounds, boolean/scientific notation parsing, bytes/base64, UUID parsing,
  contract parameter arrays, addresses (hex/base58/array errors), and signer scope/flat
  signer+witness parsing plus invalid signer entries, base58 signer accounts, signer limits/null
  entries, and witness base64 validation.
- ✔ Added RPC parameter conversion coverage for unsigned underflow, negative block indices, and
  short-hash contract identifiers (treated as names, hash accessor invalid).
- ✔ Added RPC parameter conversion coverage for block hash string parsing (0x and raw) and
  numeric-string contract identifiers.
- ✔ Added RPC parameter conversion coverage for numeric block index strings and UInt160 hash
  strings as contract identifiers.
- ✔ Added RPC parameter conversion parity for numeric boolean casting, unicode digits, and large double rejection.
- ✔ Added RPC smart-contract tests for invalid params, iterator/session errors, and invalid address handling.
- ✔ Added invokeFunction positive-path tests for NEO `totalSupply` and `symbol` script/stack parity.
- ✔ Added invokeScript positive-path test for NEO `totalSupply` script/stack parity.
- ✔ Added invokeScript positive-path test for NEO `transfer` false-return parity without witnesses.
- ✔ Added invokeScript diagnostics coverage for invoked contract tree and storage change shape.
- ✔ Added invokeScript fault-state ABORT exception parity coverage.
- ✔ Added iterator traversal test for session-enabled RPC (traverse + terminate flow).
- ✔ Added invokeScript gas-limit fault coverage (incl. "Insufficient GAS" exception) and session-expiration iterator rejection coverage.
- ✔ Aligned `terminatesession` RPC to return `false` for unknown sessions (instead of error), matching C#.
- ✔ Added invokeFunction wallet tests for signed transactions and pending signatures.
- ✔ Added invokeFunction wallet missing-account exception parity coverage.
- ✔ Added invokeFunction invalid signer account + invalid witness base64 rejection coverage.
- ✔ Added invokeFunction fault-state coverage for missing methods.
- ✔ Added invokeContractVerify coverage for invalid hash, missing verify overloads, and successful execution of a deployed verify-only contract.
- ✔ Fixed getunclaimedgas to return base58 address and added response shape test.
- ✔ Added state service RPC tests for invalid params, unknown state roots, and state trie reads.
- ✔ Aligned RPC client method-token JSON serialization to emit named CallFlags
  (e.g., "All") and added parity coverage mirroring UT_RpcModels.
- ✔ Added RPC client `RpcInvokeResult`/`RpcStack` JSON serialization helpers and
  tests to mirror UT_RpcModels output shape.
- ✔ Aligned RPC client stack item parsing for InteropInterface to accept raw JSON
  payloads (Utility.StackItemFromJson parity).
- ✔ Added RPC client `RpcApplicationLog`/`Execution`/`RpcNotifyEventArgs` JSON
  serialization parity coverage using RpcTestCases fixtures.
- ✔ Added RPC client `RpcBlockHeader` JSON serialization parity coverage using
  RpcTestCases fixtures.
- ✔ Added RPC client `RpcBlock` JSON serialization parity coverage using
  RpcTestCases fixtures (including nonce casing and block-size varint parity).
- ✔ Added RPC client `RpcRawMemPool` JSON serialization parity coverage using
  RpcTestCases fixtures.
- ✔ Added RPC client `RpcAccount` JSON serialization parity coverage using
  RpcTestCases fixtures (including `label: null` emission).
- ✔ Added RPC client `RpcTransaction` JSON serialization parity coverage using
  RpcTestCases fixtures.
- ✔ Added RPC client `RpcContractState` and `RpcNefFile` JSON serialization parity
  coverage using RpcTestCases fixtures (includes NEF `magic` and manifest `returntype` parsing).
- ✔ Added RPC client `RpcPeers` and `RpcVersion` JSON serialization parity coverage
  using RpcTestCases fixtures.
- ✔ Added RPC client `RpcNep17Balances` and `RpcNep17Transfers` JSON serialization
  parity coverage using RpcTestCases fixtures (including null transfer address).
- ✔ Added RPC client `RpcValidateAddressResult` and `RpcPlugin` JSON serialization
  parity coverage using RpcTestCases fixtures.
- ✔ Added RPC client `RpcTransferOut` and `RpcValidator` JSON serialization parity
  coverage using RpcTestCases fixtures.
- ✔ Added RPC client `RpcUnclaimedGas` JSON serialization parity coverage using
  RpcTestCases fixtures.
- ✔ Added RPC client `RpcInvokeResult` JSON serialization parity coverage using
  RpcTestCases fixtures.
- ✔ Added RPC client `RpcRequest` and `RpcResponse` JSON serialization parity
  coverage using RpcTestCases fixtures.
- ✔ Aligned RPC client `send_raw_transaction` to use base64 payloads and return
  the relay hash (UInt256), plus added fixture-backed request/response coverage.
- ✔ Added RPC client `submit_block` to use base64 payloads and return block hash,
  plus added fixture-backed request/response coverage.
- ✔ Added RPC client `invoke_script` fixture-backed request/response coverage
  verifying base64 script payload and parsed gas/script values.
- ✔ Added RPC client `get_block_count` fixture-backed request/response coverage
  to validate no-params request shape and numeric parsing.
- ✔ Added RPC client `get_block_hash` and `get_block_header_count` fixture-backed
  request/response coverage to validate parameter shapes and numeric parsing.
- ✔ Aligned RPC client `get_block_hex`/`get_block_header_hex` to omit verbose
  parameters and added fixture-backed request/response coverage.
- ✔ Added RPC client `get_raw_mempool`/`get_raw_mempool_both` methods and
  fixture-backed request/response coverage for array/object shapes.
- ✔ Added RPC client `get_raw_transaction_hex` and fixture-backed coverage for
  non-verbose `getrawtransaction` responses.
- ✔ Added RPC client methods and fixture-backed request/response coverage for
  `getbestblockhash`, `getblock`/`getblockheader` verbose, `getrawtransaction`
  verbose, `invokefunction`, `getcontractstate`, `getpeers`, `getversion`,
  `getapplicationlog` (with trigger), `getunclaimedgas`, `importprivkey`,
  `validateaddress`, and NEP17 transfers/balances.
- ✔ Added RPC client `calculate_network_fee` and TransactionManager parity
  coverage for signature/multi-sig signing, add-witness flows, and fee checks
  (including insufficient GAS handling and duplicate signature rejection).
- ✔ Added RPC client `get_storage`, `get_connection_count`, `get_committee`, and
  `get_next_block_validators` plus fixture-backed request/response coverage.
- ✔ Added RPC client `get_transaction_height`, `get_native_contracts`, and
  `listplugins` fixture-backed request/response coverage.
- ✔ Added RPC client `SendAsync` error/no-throw response parity coverage for
  `sendrawtransaction` error responses.
- ✔ Added RPC client basic-auth header parity coverage (Authorization header set
  with `user:pass` base64) to mirror UT_RpcClient constructor behavior.
- ✔ Added RPC client fixture coverage for hash/index multi-cases and numeric
  contract IDs in `getcontractstate`/`getstorage`, plus invokescript variants.
- ✔ Aligned RPC client hash-or-index parsing to accept negative indices (C#
  int parsing parity) with request-shape coverage.
- ✔ Aligned RPC client hash-or-index parsing to trim numeric input (C# int
  parsing whitespace tolerance) with request-shape coverage.
- ✔ Aligned RPC client response token conversions with Neo.Json `AsString`/
  `AsNumber`/`AsBoolean` semantics (string/number/boolean coercion).
- ✔ Aligned `gettransactionheight` parsing to match C# `AsString` conversion
  behavior for numeric JSON responses.
- ✔ Aligned numeric token parsing for string values (empty → 0, invalid → NaN)
  to mirror Neo.Json `AsNumber` behavior.
- ✔ Added MethodToken serialization/deserialization parity tests (UT_MethodToken).
- ✔ Added StorageIterator dispose/value parity tests (UT_StorageIterator).
- ✔ Added ApplicationEngine Contract syscall parity tests for CreateStandardAccount and
  CreateMultisigAccount (UT_ApplicationEngine.Contract).
- ✔ Added ApplicationEngine Runtime parity tests for GetRandom (Aspidochelone path),
  invalid UTF-8 log handling, and notify parameter validation (UT_ApplicationEngine.Runtime).
- ✔ Added Runtime notify immutable cloning to coerce Buffer -> ByteString and a
  circular-reference guard to match notification behavior (UT_ApplicationEngine.Runtime).
- ✔ Confirmed InteropService NEO parity via existing crypto syscall, contract management,
  and storage find tests (UT_InteropService.NEO).
- ✔ Added RPC client wallet methods (`open_wallet`, `close_wallet`, `get_new_address`,
  `dump_priv_key`, `list_address`, `get_wallet_balance`, `get_wallet_unclaimed_gas`,
  `send_from`, `send_to_address`, `send_many`) with fixture-backed coverage.
- ✔ Added RPC client `get_block_sys_fee` with fixture-backed request/response coverage.
- ✔ Added RPC client StateApi request/response coverage for state service endpoints
  (`getstateroot`, `getstateheight`, `getstate`, `getproof`, `verifyproof`, `findstates`).
- ✔ Added RPC client PolicyApi mocked coverage for fee factor, storage price, fee per
  byte, and blocked-account checks.
- ✔ Added RPC client WalletApi coverage for unclaimed GAS, token/NEO/GAS balances,
  and account-state aggregation (mocked invokescript/getblockcount).
- ✔ Added WalletApi parity for claim/transfer flows to relay transactions,
  support optional ASSERT emission in NEP-17 transfer scripts, await
  confirmations with timeout handling, and cover multi-sig transfer paths.
- ✔ Added WalletApi multi-sig transfer parity coverage for empty-string `data`
  payloads (matches C# `TransferAsync` behavior).
- ✔ Added WalletApi parity for WIF/private-key claim GAS overload and decimal
  transfer amount conversion (string key + decimals lookup).
- ✔ Added RPC utility stack-item JSON parity coverage for ByteString/Buffer,
  Pointer (string numeric), and Any/null handling.
- ✔ Aligned RPC utility stack-item JSON fallback for unknown types to mirror C#
  (string/null) behavior and added tests.
- ✔ Added RPC utility `RuleFromJson` parity coverage for witness rules and
  witness condition parsing (Boolean/Not/And/Or/Group/CalledBy*).
- ✔ Added BigDecimal `ToBigInteger` parity (decimal scaling with precision guard),
  aligned with UT_Utility conversion expectations.
- ✔ Added RpcInvokeResult parsing coverage for unknown stack-item types to ensure
  fallback values are preserved.
- ✔ Added RpcInvokeResult circular stack serialization parity (returns
  "error: recursive reference") to mirror C# behavior.
- ✔ Aligned StackItem JSON parsing for `Any` to mirror C# fallback to string
  values (or null) and added coverage.
- ✔ Added RPC utility coverage for key-pair parsing (WIF/hex/0x) and script-hash
  parsing error cases to mirror C# UT_Utility expectations.
- ✔ Aligned `RpcUtility::as_script_hash` with C# native contract name/id mapping
  and added unit test coverage.
- ✔ Aligned NEP17 `GetTokenInfo` client API to fetch contract state name (C# parity)
  and added mocked RPC tests exercising `getcontractstate` + `invokescript`,
  including the string overload accepting contract names.
- ✔ Added mocked NEP17 API tests for `balanceOf`, `symbol`, `decimals`, and
  `totalSupply` to mirror C# API surface behavior.
- ✔ Added mocked NEP17 transfer transaction coverage for explicit `from` and
  multi-sig creation paths, plus insufficient-key error handling.
- ✔ Aligned NEP17 transfer/invoke script construction to include `CallFlags::ALL`
  and append `ASSERT`, matching C# `MakeScript` + `addAssert` behavior.
- ✔ Added a NEP17 transfer script unit test to validate `CallFlags::ALL` and
  `ASSERT` emission.
- ✔ Added a NEP17 transfer script unit test to validate empty-string data payload
  handling for `CreateTransferTx` parity.
- ✔ Added a NEP17 `make_script` unit test to validate `CallFlags::ALL` emission
  and CreateArray/pack behavior for argument handling.
- ✔ Added a ContractClient dynamic call script unit test to validate
  `EmitDynamicCall` byte layout parity.
- ✔ Added a ContractClient dynamic call script unit test for packed arguments
  (CreateArray/pack ordering parity).
- ✔ Added mocked ContractClient parity coverage for `TestInvoke` (ByteString
  integer decoding) and `CreateDeployContractTx` transaction creation.
- ✔ Added ContractClient invoke parsing coverage for Map and Struct stack items.

## Next Actions
1. Add full RPC client integration tests once a runnable node fixture is
   available in CI.
2. Continue sweeping remaining non-consensus parity gaps (ledger/VM/RPC) beyond VM JSON stack verification.
