# Neo Rust TODO Porting Map

This document catalogs outstanding TODO markers in the Rust codebase and provides the corresponding C# implementation under `neo_csharp/` to guide parity work.

## Ledger

- `crates/neo/src/ledger/blockchain.rs:39` – Reload persisted blocks/transactions into in-memory caches after persistence.
  - C# reference: `neo_csharp/src/Neo/Ledger/Blockchain.cs` (`Persist` method updates `_system.MemPool`, clears `_blockCache`, refreshes `_extensibleWitnessWhiteList`, and publishes `PersistCompleted`).
  - Notes: Wire this once the Rust persistence pipeline completes block processing and mempool integration.

- `crates/neo/src/ledger/transaction_verification_context.rs:141` – Replace fallback balance provider with GAS native contract query.
  - C# reference: `neo_csharp/src/Neo/Ledger/TransactionVerificationContext.cs` (`CheckTransaction` calls `NativeContract.GAS.BalanceOf`).
  - Notes: Requires native contract bindings and store snapshot access for GAS balance checks.

## Network / P2P Core

- `crates/neo/src/network/p2p/remote_node.rs:442` – Forward received transactions into the mempool/transaction router.
  - C# reference: `neo_csharp/src/Neo/Network/P2P/RemoteNode.ProtocolHandler.cs` (`OnInventoryReceived` pushes transactions to `_system.TxRouter` after conflict checks).

- `crates/neo/src/network/p2p/remote_node.rs:451` – Forward received blocks to the blockchain actor.
  - C# reference: `neo_csharp/src/Neo/Network/P2P/RemoteNode.ProtocolHandler.cs` (`OnInventoryReceived` and `OnInvMessageReceived` relay blocks to `_system.Blockchain`).

## Network Payload Verification

- `crates/neo/src/network/p2p/payloads/conflicts.rs:31` – Verify conflicting transactions reside on-chain when cache methods are ready.
  - C# reference: `neo_csharp/src/Neo/Network/P2P/Payloads/Conflicts.cs` (`Verify` queries the snapshot for conflict hashes).

- `crates/neo/src/network/p2p/payloads/extensible_payload.rs:71` – Use actual ledger height for validity window checks.
  - C# reference: `neo_csharp/src/Neo/Network/P2P/Payloads/ExtensiblePayload.cs` (`Verify` compares against `NativeContract.Ledger.CurrentIndex`).

- `crates/neo/src/network/p2p/payloads/high_priority_attribute.rs:28` – Ensure signers include the committee address.
  - C# reference: `neo_csharp/src/Neo/Network/P2P/Payloads/HighPriorityAttribute.cs` (`Verify` pulls committee address from snapshot).

- `crates/neo/src/network/p2p/payloads/not_valid_before.rs:31` – Check current block height to enforce `NotValidBefore` attribute.
  - C# reference: `neo_csharp/src/Neo/Network/P2P/Payloads/NotValidBefore.cs` (`Verify` uses `NativeContract.Ledger.CurrentIndex`).

- `crates/neo/src/network/p2p/payloads/signer.rs:142` – Include witness rules in signer JSON.
  - C# reference: `neo_csharp/src/Neo/Network/P2P/Payloads/Signer.cs` (`ToJson` serializes `WitnessRules`).

- `crates/neo/src/network/p2p/payloads/signer.rs:157` – Implement JSON deserialization for signers.
  - C# reference: `neo_csharp/src/Neo/Network/P2P/Payloads/Signer.cs` (`FromJson` parses signer scopes, accounts, and rules).

- `crates/neo/src/network/p2p/payloads/transaction_attribute.rs:85` – Implement attribute-specific verification logic.
  - C# reference: `neo_csharp/src/Neo/Network/P2P/Payloads/TransactionAttribute.cs` (`Verify` routes by attribute type).

- `crates/neo/src/network/p2p/payloads/transaction_attribute.rs:93` – Retrieve attribute fee from Policy contract.
  - C# reference: `neo_csharp/src/Neo/Network/P2P/Payloads/TransactionAttribute.cs` (`CalculateNetworkFee` uses `NativeContract.Policy.GetAttributeFee`).

- `crates/neo/src/network/p2p/payloads/transaction_attribute.rs:105` – Populate attribute-specific JSON fields.
  - C# reference: `neo_csharp/src/Neo/Network/P2P/Payloads/TransactionAttribute.cs` (`ToJson` emits detail per attribute).

- `crates/neo/src/network/p2p/payloads/transaction.rs` – Remaining port items:
  - L441: Implement address/JSON conversion for sender scripts.
    - C# reference: `neo_csharp/src/Neo/Network/P2P/Payloads/Transaction.cs` (`ToJson` maps sender & addresses).
  - L455/L456/L458/L459: Serialize signers, attributes, witnesses to JSON.
    - C# reference: same file (`ToJson`).
  - L485: Import native contract references when available.
    - C# reference: uses `NativeContract.*` in `Verify`.
  - L488: Obtain current block height via ledger contract.
    - C# reference: `NativeContract.Ledger.CurrentIndex`.
  - L496/L497: Check Policy contract for blocked accounts.
    - C# reference: Policy native contract `IsBlocked`. 
  - L504/L505: Integrate `TransactionVerificationContext.CheckTransaction` for conflict handling.
  - L511: Execute attribute verification while calculating fees.
  - L523: Pull fee-per-byte from Policy contract.
  - L537: Pull execution fee factor from Policy contract.
  - L543: Verify witnesses via helper utilities.
  - L582: Validate script using VM `Script` loader.
  - L588: Use `GetScriptHashesForVerifying` for witness checks.
  - L593: Perform signature verification via helper (e.g., `Helper.IsSignatureContract`).
  - L644: Execute `ApplicationEngine` for on-chain verification when available.
  - L781: Implement `InteropInterface` for transactions.
  - L816: Implement `Hash` trait equivalent to C# `IEquatable`/`GetHashCode`.
  - C# reference for all: `neo_csharp/src/Neo/Network/P2P/Payloads/Transaction.cs` and related helper classes under `Neo.SmartContract` and `Neo.Cryptography`.

## Wallets

- `crates/neo/src/wallets/key_pair.rs:20` – Re-enable Zeroize imports.
  - C# reference: sensitive key handling occurs implicitly; Rust should mirror secure memory patterns as in other crates.
  - Notes: Align with Rust `zeroize` usage once module layout settles.

- `crates/neo/src/wallets/key_pair.rs:24` – Derive `Zeroize`, `ZeroizeOnDrop` for KeyPair.
  - Notes: Mirror secure disposal semantics required by the C# implementation’s `KeyPair` class.

- `crates/neo/src/wallets/nep6.rs:6` and `:15` – Restore NEP-6 wallet imports and version reference.
  - C# reference: `neo_csharp/src/Neo/Wallets/NEP6/NEP6Wallet.cs` and related types (`VerificationContract`, `KeyPair`, `Contract`).

- `crates/neo/src/wallets/wallet.rs:6` – Restore wallet module imports post restructuring.
  - C# reference: `neo_csharp/src/Neo/Wallets/Wallet.cs`.

## Oracle Plugin

- `crates/plugins/src/oracle_service/oracle_service.rs:221` – Add timestamp-based pruning logic for active requests.
  - C# reference: `neo_csharp/src/Plugins/OracleService/OracleService.cs` (filters requests by expiration timestamp).

## REST Server Plugin (placeholders)

Port the REST server plugin components from `neo_csharp/src/Plugins/RestServer/`:

### Authentication

- `crates/plugins/src/rest_server/authentication/basic_authentication_handler.rs` ← `Authentication/BasicAuthenticationHandler.cs`.

### Binder

- `crates/plugins/src/rest_server/binder/uint160_binder_provider.rs` ← `Binder/UInt160BinderProvider.cs`.
- `crates/plugins/src/rest_server/binder/uint160_binder.rs` ← `Binder/UInt160Binder.cs`.

### Controllers (v1)

- `crates/plugins/src/rest_server/controllers/v1/contracts_controller.rs` ← `Controllers/v1/ContractsController.cs`.
- `crates/plugins/src/rest_server/controllers/v1/ledger_controller.rs` ← `Controllers/v1/LedgerController.cs`.
- `crates/plugins/src/rest_server/controllers/v1/tokens_controller.rs` ← `Controllers/v1/TokensController.cs`.

### Exceptions

- `crates/plugins/src/rest_server/exceptions/application_engine_exception.rs` ← `Exceptions/ApplicationEngineException.cs`.
- `crates/plugins/src/rest_server/exceptions/json_property_null_or_empty_exception.rs` ← `Exceptions/JsonPropertyNullOrEmptyException.cs`.
- `crates/plugins/src/rest_server/exceptions/nep11_not_supported_exception.rs` ← `Exceptions/Nep11NotSupportedException.cs`.
- `crates/plugins/src/rest_server/exceptions/nep17_not_supported_exception.rs` ← `Exceptions/Nep17NotSupportedException.cs`.
- `crates/plugins/src/rest_server/exceptions/node_exception.rs` ← `Exceptions/NodeException.cs`.
- `crates/plugins/src/rest_server/exceptions/uint256_format_exception.rs` ← `Exceptions/UInt256FormatException.cs`.

### Extensions

- `crates/plugins/src/rest_server/extensions/ledger_contract_extensions.rs` ← `Extensions/LedgerContractExtensions.cs`.
- `crates/plugins/src/rest_server/extensions/uint160_extensions.rs` ← `Extensions/UInt160Extensions.cs`.

### Helpers

- `crates/plugins/src/rest_server/helpers/contract_helper.rs` ← `Helpers/ContractHelper.cs`.
- `crates/plugins/src/rest_server/helpers/script_helper.rs` ← `Helpers/ScriptHelper.cs`.

### Middleware

- `crates/plugins/src/rest_server/middleware/rest_server_middleware.rs` ← `Middleware/RestServerMiddleware.cs`.

### Models

- `crates/plugins/src/rest_server/models/error/parameter_format_exception_model.rs` ← `Models/Error/ParameterFormatExceptionModel.cs`.
- `crates/plugins/src/rest_server/models/ledger/memory_pool_count_model.rs` ← `Models/Ledger/MemoryPoolCountModel.cs`.
- `crates/plugins/src/rest_server/models/token/nep11_token_model.rs` ← `Models/Token/NEP11TokenModel.cs`.
- `crates/plugins/src/rest_server/models/token/nep17_token_model.rs` ← `Models/Token/NEP17TokenModel.cs`.
- `crates/plugins/src/rest_server/models/token/token_balance_model.rs` ← `Models/Token/TokenBalanceModel.cs`.

### Newtonsoft JSON Converters

- `crates/plugins/src/rest_server/newtonsoft/json/big_decimal_json_converter.rs` ← `Newtonsoft/Json/BigDecimalJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/block_header_json_converter.rs` ← `Newtonsoft/Json/BlockHeaderJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/block_json_converter.rs` ← `Newtonsoft/Json/BlockJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/contract_abi_json_converter.rs` ← `Newtonsoft/Json/ContractAbiJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/contract_event_descriptor_json_converter.rs` ← `Newtonsoft/Json/ContractEventDescriptorJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/contract_group_json_converter.rs` ← `Newtonsoft/Json/ContractGroupJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/contract_invoke_parameters_json_converter.rs` ← `Newtonsoft/Json/ContractInvokeParametersJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/contract_json_converter.rs` ← `Newtonsoft/Json/ContractJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/contract_manifest_json_converter.rs` ← `Newtonsoft/Json/ContractManifestJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/contract_method_json_converter.rs` ← `Newtonsoft/Json/ContractMethodJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/contract_method_parameters_json_converter.rs` ← `Newtonsoft/Json/ContractMethodParametersJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/contract_parameter_definition_json_converter.rs` ← `Newtonsoft/Json/ContractParameterDefinitionJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/contract_parameter_json_converter.rs` ← `Newtonsoft/Json/ContractParameterJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/contract_permission_descriptor_json_converter.rs` ← `Newtonsoft/Json/ContractPermissionDescriptorJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/contract_permission_json_converter.rs` ← `Newtonsoft/Json/ContractPermissionJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/ec_point_json_converter.rs` ← `Newtonsoft/Json/ECPointJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/guid_json_converter.rs` ← `Newtonsoft/Json/GuidJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/interop_interface_json_converter.rs` ← `Newtonsoft/Json/InteropInterfaceJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/method_token_json_converter.rs` ← `Newtonsoft/Json/MethodTokenJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/nef_file_json_converter.rs` ← `Newtonsoft/Json/NefFileJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/read_only_memory_bytes_json_converter.rs` ← `Newtonsoft/Json/ReadOnlyMemoryBytesJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/signer_json_converter.rs` ← `Newtonsoft/Json/SignerJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/stack_item_json_converter.rs` ← `Newtonsoft/Json/StackItemJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/transaction_attribute_json_converter.rs` ← `Newtonsoft/Json/TransactionAttributeJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/transaction_json_converter.rs` ← `Newtonsoft/Json/TransactionJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/uint160_json_converter.rs` ← `Newtonsoft/Json/UInt160JsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/uint256_json_converter.rs` ← `Newtonsoft/Json/UInt256JsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/vm_array_json_converter.rs` ← `Newtonsoft/Json/VmArrayJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/vm_boolean_json_converter.rs` ← `Newtonsoft/Json/VmBooleanJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/vm_buffer_json_converter.rs` ← `Newtonsoft/Json/VmBufferJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/vm_byte_string_json_converter.rs` ← `Newtonsoft/Json/VmByteStringJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/vm_integer_json_converter.rs` ← `Newtonsoft/Json/VmIntegerJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/vm_map_json_converter.rs` ← `Newtonsoft/Json/VmMapJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/vm_null_json_converter.rs` ← `Newtonsoft/Json/VmNullJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/vm_pointer_json_converter.rs` ← `Newtonsoft/Json/VmPointerJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/vm_struct_json_converter.rs` ← `Newtonsoft/Json/VmStructJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/witness_json_converter.rs` ← `Newtonsoft/Json/WitnessJsonConverter.cs`.
- `crates/plugins/src/rest_server/newtonsoft/json/witness_rule_json_converter.rs` ← `Newtonsoft/Json/WitnessRuleJsonConverter.cs`.

### Tokens

- `crates/plugins/src/rest_server/tokens/nep11_token.rs` ← `Tokens/NEP11Token.cs`.
- `crates/plugins/src/rest_server/tokens/nep17_token.rs` ← `Tokens/NEP17Token.cs`.

## Tokens Tracker Plugin

- `crates/plugins/src/tokens_tracker/trackers/nep_11/nep11_tracker.rs` – Port NEP-11 tracker logic.
  - C# reference: `neo_csharp/src/Plugins/TokensTracker/Trackers/NEP-11/Nep11Tracker.cs`.

- `crates/plugins/src/tokens_tracker/trackers/nep_17/nep17_tracker.rs` – Port NEP-17 tracker logic.
  - C# reference: `neo_csharp/src/Plugins/TokensTracker/Trackers/NEP-17/Nep17Tracker.cs`.

## RPC Client

- `crates/rpc_client/src/utility.rs:179` – Implement block deserialization from JSON.
  - C# reference: `neo_csharp/src/RpcClient/Utility.cs` (`BlockFromJson`).

- `crates/rpc_client/src/utility.rs:228` – Implement transaction deserialization from JSON.
  - C# reference: `neo_csharp/src/RpcClient/Utility.cs` (`TransactionFromJson`).

- `crates/rpc_client/src/utility.rs:290` – Convert witness conditions to JSON.
  - C# reference: `neo_csharp/src/RpcClient/Utility.cs` (`WitnessConditionToJson`).

- `crates/rpc_client/src/models/rpc_block_header.rs:32` – Implement RPC block header model deserialization.
  - C# reference: `neo_csharp/src/RpcClient/Models/RpcBlockHeader.cs`.

- `crates/rpc_client/src/models/rpc_raw_mem_pool.rs:1` – Fill in `RpcRawMemPool` model.
  - C# reference: `neo_csharp/src/RpcClient/Models/RpcRawMemPool.cs`.
- **Step 1 (done)**: restore `dbft_plugin` stub and gate the full port behind `dbft-full`.
- **Step 2**: expose missing system/mempool interfaces (`NeoSystem::try_get_transaction`, sorted mempool access, consensus store wiring) and adjust consensus service to use them.
- **Step 3**: refactor the consensus handlers (`on_prepare_request`, `on_prepare_response`, `on_commit`, etc.) to satisfy borrow rules, eliminate recursive async calls, and re-enable the real plugin.
