# Neo Port Parity Report

- Matched Rust files: 67
- Skipped C# files (excluded metadata): 74
- Missing C# files without Rust counterpart: 582

## Missing Coverage by Module

| Module prefix | Missing files |
| ------------- | ------------- |
| Plugins/RestServer | 92 |
| Neo.VM | 39 |
| RpcClient | 35 |
| Neo/SmartContract | 29 |
| Neo/Network/P2P/Payloads | 28 |
| Neo.Cryptography.BLS12_381 | 26 |
| Neo.CLI | 22 |
| Plugins/RpcServer | 22 |
| Plugins/DBFTPlugin | 20 |
| Neo/Network/P2P | 18 |
| Neo/Persistence | 14 |
| Neo/SmartContract/Native | 14 |
| Plugins/ApplicationLogs | 14 |
| Plugins/LevelDBStore | 14 |
| Neo.Json | 12 |
| Plugins/TokensTracker | 12 |
| Neo.Extensions | 11 |
| Neo/Network/P2P/Payloads/Conditions | 11 |
| Neo/IEventHandlers | 10 |
| Neo/Wallets | 10 |
| Plugins/StateService | 10 |
| Neo.Cryptography.MPTTrie | 9 |
| Plugins/SQLiteWallet | 9 |
| Neo.ConsoleService | 8 |
| Neo/Ledger | 8 |
| Neo | 7 |
| Neo/SmartContract/ApplicationEngine | 6 |
| Plugins/OracleService | 6 |
| Neo/Builders | 5 |
| Neo/Extensions | 5 |
| Neo/SmartContract/Manifest | 5 |
| Neo.IO/Caching | 4 |
| Neo/Cryptography | 4 |
| Neo/Extensions/IO | 4 |
| Neo/Extensions/SmartContract | 4 |
| Plugins/RocksDBStore | 4 |
| Neo/Cryptography/ECC | 3 |
| Neo/Extensions/VM | 3 |
| Neo/Sign | 3 |
| Plugins/SignClient | 3 |
| Neo.Extensions/Collections | 2 |
| Neo.IO | 2 |
| Neo/IO/Caching | 2 |
| Neo/SmartContract/Interop | 2 |
| Neo/SmartContract/Iterators | 2 |
| Plugins/StorageDumper | 2 |
| Neo.Extensions/Exceptions | 1 |
| Neo.Extensions/Factories | 1 |
| Neo.Extensions/Net | 1 |
| Neo.IO/Actors | 1 |
| Neo/Extensions/Collections | 1 |
| Neo/Network | 1 |
| Neo/SmartContract/Json | 1 |

## Detailed Missing Files by Module

### Plugins/RestServer
- Plugins/RestServer/Authentication/BasicAuthenticationHandler.cs
- Plugins/RestServer/Binder/UInt160Binder.cs
- Plugins/RestServer/Binder/UInt160BinderProvider.cs
- Plugins/RestServer/Controllers/v1/ContractsController.cs
- Plugins/RestServer/Controllers/v1/LedgerController.cs
- Plugins/RestServer/Controllers/v1/NodeController.cs
- Plugins/RestServer/Controllers/v1/TokensController.cs
- Plugins/RestServer/Controllers/v1/UtilsController.cs
- Plugins/RestServer/Exceptions/AddressFormatException.cs
- Plugins/RestServer/Exceptions/ApplicationEngineException.cs
- Plugins/RestServer/Exceptions/BlockNotFoundException.cs
- Plugins/RestServer/Exceptions/ContractNotFoundException.cs
- Plugins/RestServer/Exceptions/InvalidParameterRangeException.cs
- Plugins/RestServer/Exceptions/JsonPropertyNullOrEmptyException.cs
- Plugins/RestServer/Exceptions/Nep11NotSupportedException.cs
- Plugins/RestServer/Exceptions/Nep17NotSupportedException.cs
- Plugins/RestServer/Exceptions/NodeException.cs
- Plugins/RestServer/Exceptions/NodeNetworkException.cs
- Plugins/RestServer/Exceptions/QueryParameterNotFoundException.cs
- Plugins/RestServer/Exceptions/RestErrorCodes.cs
- Plugins/RestServer/Exceptions/ScriptHashFormatException.cs
- Plugins/RestServer/Exceptions/TransactionNotFoundException.cs
- Plugins/RestServer/Exceptions/UInt256FormatException.cs
- Plugins/RestServer/Extensions/LedgerContractExtensions.cs
- Plugins/RestServer/Extensions/ModelExtensions.cs
- Plugins/RestServer/Extensions/UInt160Extensions.cs
- Plugins/RestServer/Helpers/ContractHelper.cs
- Plugins/RestServer/Helpers/ScriptHelper.cs
- Plugins/RestServer/Middleware/RestServerMiddleware.cs
- Plugins/RestServer/Models/Blockchain/AccountDetails.cs
- Plugins/RestServer/Models/Contract/InvokeParams.cs
- Plugins/RestServer/Models/CountModel.cs
- Plugins/RestServer/Models/Error/ErrorModel.cs
- Plugins/RestServer/Models/Error/ParameterFormatExceptionModel.cs
- Plugins/RestServer/Models/ExecutionEngineModel.cs
- Plugins/RestServer/Models/Ledger/MemoryPoolCountModel.cs
- Plugins/RestServer/Models/Node/PluginModel.cs
- Plugins/RestServer/Models/Node/ProtocolSettingsModel.cs
- Plugins/RestServer/Models/Node/RemoteNodeModel.cs
- Plugins/RestServer/Models/Token/NEP11TokenModel.cs
- Plugins/RestServer/Models/Token/NEP17TokenModel.cs
- Plugins/RestServer/Models/Token/TokenBalanceModel.cs
- Plugins/RestServer/Models/Utils/UtilsAddressIsValidModel.cs
- Plugins/RestServer/Models/Utils/UtilsAddressModel.cs
- Plugins/RestServer/Models/Utils/UtilsScriptHashModel.cs
- Plugins/RestServer/Newtonsoft/Json/BigDecimalJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/BlockHeaderJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/BlockJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/ContractAbiJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/ContractEventDescriptorJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/ContractGroupJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/ContractInvokeParametersJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/ContractJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/ContractManifestJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/ContractMethodJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/ContractMethodParametersJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/ContractParameterDefinitionJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/ContractParameterJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/ContractPermissionDescriptorJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/ContractPermissionJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/ECPointJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/GuidJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/InteropInterfaceJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/MethodTokenJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/NefFileJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/ReadOnlyMemoryBytesJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/SignerJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/StackItemJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/TransactionAttributeJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/TransactionJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/UInt160JsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/UInt256JsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/VmArrayJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/VmBooleanJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/VmBufferJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/VmByteStringJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/VmIntegerJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/VmMapJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/VmNullJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/VmPointerJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/VmStructJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/WitnessConditionJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/WitnessJsonConverter.cs
- Plugins/RestServer/Newtonsoft/Json/WitnessRuleJsonConverter.cs
- Plugins/RestServer/Providers/BlackListControllerFeatureProvider.cs
- Plugins/RestServer/RestServerPlugin.cs
- Plugins/RestServer/RestServerSettings.cs
- Plugins/RestServer/RestServerUtility.JTokens.cs
- Plugins/RestServer/RestServerUtility.cs
- Plugins/RestServer/RestWebServer.cs
- Plugins/RestServer/Tokens/NEP11Token.cs
- Plugins/RestServer/Tokens/NEP17Token.cs

### Neo.VM
- Neo.VM/BadScriptException.cs
- Neo.VM/CatchableException.cs
- Neo.VM/Collections/OrderedDictionary.cs
- Neo.VM/ExceptionHandlingContext.cs
- Neo.VM/ExceptionHandlingState.cs
- Neo.VM/ExecutionContext.SharedStates.cs
- Neo.VM/ExecutionEngineLimits.cs
- Neo.VM/GlobalSuppressions.cs
- Neo.VM/IReferenceCounter.cs
- Neo.VM/JumpTable/JumpTable.Bitwisee.cs
- Neo.VM/JumpTable/JumpTable.Compound.cs
- Neo.VM/JumpTable/JumpTable.Control.cs
- Neo.VM/JumpTable/JumpTable.Numeric.cs
- Neo.VM/JumpTable/JumpTable.Push.cs
- Neo.VM/JumpTable/JumpTable.Slot.cs
- Neo.VM/JumpTable/JumpTable.Splice.cs
- Neo.VM/JumpTable/JumpTable.Stack.cs
- Neo.VM/JumpTable/JumpTable.Types.cs
- Neo.VM/JumpTable/JumpTable.cs
- Neo.VM/OpCode.cs
- Neo.VM/OperandSizeAttribute.cs
- Neo.VM/Slot.cs
- Neo.VM/Types/Array.cs
- Neo.VM/Types/Boolean.cs
- Neo.VM/Types/Buffer.cs
- Neo.VM/Types/ByteString.cs
- Neo.VM/Types/CompoundType.cs
- Neo.VM/Types/Integer.cs
- Neo.VM/Types/InteropInterface.cs
- Neo.VM/Types/Map.cs
- Neo.VM/Types/Null.cs
- Neo.VM/Types/Pointer.cs
- Neo.VM/Types/PrimitiveType.cs
- Neo.VM/Types/StackItem.Vertex.cs
- Neo.VM/Types/StackItem.cs
- Neo.VM/Types/StackItemType.cs
- Neo.VM/Types/Struct.cs
- Neo.VM/VMState.cs
- Neo.VM/VMUnhandledException.cs

### RpcClient
- RpcClient/ContractClient.cs
- RpcClient/Models/RpcAccount.cs
- RpcClient/Models/RpcApplicationLog.cs
- RpcClient/Models/RpcBlock.cs
- RpcClient/Models/RpcBlockHeader.cs
- RpcClient/Models/RpcContractState.cs
- RpcClient/Models/RpcFoundStates.cs
- RpcClient/Models/RpcInvokeResult.cs
- RpcClient/Models/RpcMethodToken.cs
- RpcClient/Models/RpcNefFile.cs
- RpcClient/Models/RpcNep17Balances.cs
- RpcClient/Models/RpcNep17TokenInfo.cs
- RpcClient/Models/RpcNep17Transfers.cs
- RpcClient/Models/RpcPeers.cs
- RpcClient/Models/RpcPlugin.cs
- RpcClient/Models/RpcRawMemPool.cs
- RpcClient/Models/RpcRequest.cs
- RpcClient/Models/RpcResponse.cs
- RpcClient/Models/RpcStateRoot.cs
- RpcClient/Models/RpcTransaction.cs
- RpcClient/Models/RpcTransferOut.cs
- RpcClient/Models/RpcUnclaimedGas.cs
- RpcClient/Models/RpcValidateAddressResult.cs
- RpcClient/Models/RpcValidator.cs
- RpcClient/Models/RpcVersion.cs
- RpcClient/Nep17API.cs
- RpcClient/PolicyAPI.cs
- RpcClient/Properties/AssemblyInfo.cs
- RpcClient/RpcClient.cs
- RpcClient/RpcException.cs
- RpcClient/StateAPI.cs
- RpcClient/TransactionManager.cs
- RpcClient/TransactionManagerFactory.cs
- RpcClient/Utility.cs
- RpcClient/WalletAPI.cs

### Neo/SmartContract
- Neo/SmartContract/BinarySerializer.cs
- Neo/SmartContract/CallFlags.cs
- Neo/SmartContract/Contract.cs
- Neo/SmartContract/ContractBasicMethod.cs
- Neo/SmartContract/ContractParameter.cs
- Neo/SmartContract/ContractParameterType.cs
- Neo/SmartContract/ContractParametersContext.cs
- Neo/SmartContract/ContractTask.cs
- Neo/SmartContract/ContractTaskAwaiter.cs
- Neo/SmartContract/ContractTaskMethodBuilder.cs
- Neo/SmartContract/DeployedContract.cs
- Neo/SmartContract/ExecutionContextState.cs
- Neo/SmartContract/FindOptions.cs
- Neo/SmartContract/Helper.cs
- Neo/SmartContract/IApplicationEngineProvider.cs
- Neo/SmartContract/IDiagnostic.cs
- Neo/SmartContract/IInteroperable.cs
- Neo/SmartContract/IInteroperableVerifiable.cs
- Neo/SmartContract/KeyBuilder.cs
- Neo/SmartContract/LogEventArgs.cs
- Neo/SmartContract/MaxLengthAttribute.cs
- Neo/SmartContract/MethodToken.cs
- Neo/SmartContract/NefFile.cs
- Neo/SmartContract/NotifyEventArgs.cs
- Neo/SmartContract/StorageContext.cs
- Neo/SmartContract/StorageItem.cs
- Neo/SmartContract/StorageKey.cs
- Neo/SmartContract/TriggerType.cs
- Neo/SmartContract/ValidatorAttribute.cs

### Neo/Network/P2P/Payloads
- Neo/Network/P2P/Payloads/AddrPayload.cs
- Neo/Network/P2P/Payloads/Block.cs
- Neo/Network/P2P/Payloads/Conflicts.cs
- Neo/Network/P2P/Payloads/FilterAddPayload.cs
- Neo/Network/P2P/Payloads/FilterLoadPayload.cs
- Neo/Network/P2P/Payloads/GetBlockByIndexPayload.cs
- Neo/Network/P2P/Payloads/GetBlocksPayload.cs
- Neo/Network/P2P/Payloads/HeadersPayload.cs
- Neo/Network/P2P/Payloads/HighPriorityAttribute.cs
- Neo/Network/P2P/Payloads/IInventory.cs
- Neo/Network/P2P/Payloads/IVerifiable.cs
- Neo/Network/P2P/Payloads/InvPayload.cs
- Neo/Network/P2P/Payloads/InventoryType.cs
- Neo/Network/P2P/Payloads/MerkleBlockPayload.cs
- Neo/Network/P2P/Payloads/NetworkAddressWithTime.cs
- Neo/Network/P2P/Payloads/NotValidBefore.cs
- Neo/Network/P2P/Payloads/NotaryAssisted.cs
- Neo/Network/P2P/Payloads/OracleResponse.cs
- Neo/Network/P2P/Payloads/OracleResponseCode.cs
- Neo/Network/P2P/Payloads/PingPayload.cs
- Neo/Network/P2P/Payloads/Signer.cs
- Neo/Network/P2P/Payloads/Transaction.cs
- Neo/Network/P2P/Payloads/TransactionAttribute.cs
- Neo/Network/P2P/Payloads/TransactionAttributeType.cs
- Neo/Network/P2P/Payloads/Witness.cs
- Neo/Network/P2P/Payloads/WitnessRule.cs
- Neo/Network/P2P/Payloads/WitnessRuleAction.cs
- Neo/Network/P2P/Payloads/WitnessScope.cs

### Neo.Cryptography.BLS12_381
- Neo.Cryptography.BLS12_381/Bls12.Adder.cs
- Neo.Cryptography.BLS12_381/Bls12.cs
- Neo.Cryptography.BLS12_381/ConstantTimeUtility.cs
- Neo.Cryptography.BLS12_381/Fp.cs
- Neo.Cryptography.BLS12_381/Fp12.cs
- Neo.Cryptography.BLS12_381/Fp2.cs
- Neo.Cryptography.BLS12_381/Fp6.cs
- Neo.Cryptography.BLS12_381/FpConstants.cs
- Neo.Cryptography.BLS12_381/G1Affine.cs
- Neo.Cryptography.BLS12_381/G1Constants.cs
- Neo.Cryptography.BLS12_381/G1Projective.cs
- Neo.Cryptography.BLS12_381/G2Affine.cs
- Neo.Cryptography.BLS12_381/G2Constants.cs
- Neo.Cryptography.BLS12_381/G2Prepared.Adder.cs
- Neo.Cryptography.BLS12_381/G2Prepared.cs
- Neo.Cryptography.BLS12_381/G2Projective.cs
- Neo.Cryptography.BLS12_381/GlobalSuppressions.cs
- Neo.Cryptography.BLS12_381/Gt.cs
- Neo.Cryptography.BLS12_381/GtConstants.cs
- Neo.Cryptography.BLS12_381/IMillerLoopDriver.cs
- Neo.Cryptography.BLS12_381/INumber.cs
- Neo.Cryptography.BLS12_381/MathUtility.cs
- Neo.Cryptography.BLS12_381/MillerLoopResult.cs
- Neo.Cryptography.BLS12_381/MillerLoopUtility.cs
- Neo.Cryptography.BLS12_381/Scalar.cs
- Neo.Cryptography.BLS12_381/ScalarConstants.cs

### Neo.CLI
- Neo.CLI/AssemblyExtensions.cs
- Neo.CLI/CLI/CommandLineOption.cs
- Neo.CLI/CLI/ConsolePercent.cs
- Neo.CLI/CLI/Helper.cs
- Neo.CLI/CLI/MainService.Block.cs
- Neo.CLI/CLI/MainService.Blockchain.cs
- Neo.CLI/CLI/MainService.CommandLine.cs
- Neo.CLI/CLI/MainService.Contracts.cs
- Neo.CLI/CLI/MainService.Logger.cs
- Neo.CLI/CLI/MainService.NEP17.cs
- Neo.CLI/CLI/MainService.Native.cs
- Neo.CLI/CLI/MainService.Network.cs
- Neo.CLI/CLI/MainService.Node.cs
- Neo.CLI/CLI/MainService.Plugins.cs
- Neo.CLI/CLI/MainService.Tools.cs
- Neo.CLI/CLI/MainService.Vote.cs
- Neo.CLI/CLI/MainService.Wallet.cs
- Neo.CLI/CLI/MainService.cs
- Neo.CLI/CLI/ParseFunctionAttribute.cs
- Neo.CLI/Program.cs
- Neo.CLI/Settings.cs
- Neo.CLI/Tools/VMInstruction.cs

### Plugins/RpcServer
- Plugins/RpcServer/Diagnostic.cs
- Plugins/RpcServer/Model/Address.cs
- Plugins/RpcServer/Model/BlockHashOrIndex.cs
- Plugins/RpcServer/Model/ContractNameOrHashOrId.cs
- Plugins/RpcServer/Model/SignersAndWitnesses.cs
- Plugins/RpcServer/ParameterConverter.cs
- Plugins/RpcServer/RcpServerSettings.cs
- Plugins/RpcServer/Result.cs
- Plugins/RpcServer/RpcError.cs
- Plugins/RpcServer/RpcErrorFactory.cs
- Plugins/RpcServer/RpcException.cs
- Plugins/RpcServer/RpcMethodAttribute.cs
- Plugins/RpcServer/RpcServer.Blockchain.cs
- Plugins/RpcServer/RpcServer.Node.cs
- Plugins/RpcServer/RpcServer.SmartContract.cs
- Plugins/RpcServer/RpcServer.Utilities.cs
- Plugins/RpcServer/RpcServer.Wallet.cs
- Plugins/RpcServer/RpcServer.cs
- Plugins/RpcServer/RpcServerPlugin.cs
- Plugins/RpcServer/Session.cs
- Plugins/RpcServer/Tree.cs
- Plugins/RpcServer/TreeNode.cs

### Plugins/DBFTPlugin
- Plugins/DBFTPlugin/Consensus/ConsensusContext.Get.cs
- Plugins/DBFTPlugin/Consensus/ConsensusContext.MakePayload.cs
- Plugins/DBFTPlugin/Consensus/ConsensusContext.cs
- Plugins/DBFTPlugin/Consensus/ConsensusService.Check.cs
- Plugins/DBFTPlugin/Consensus/ConsensusService.OnMessage.cs
- Plugins/DBFTPlugin/Consensus/ConsensusService.cs
- Plugins/DBFTPlugin/DBFTPlugin.cs
- Plugins/DBFTPlugin/DbftSettings.cs
- Plugins/DBFTPlugin/Messages/ChangeView.cs
- Plugins/DBFTPlugin/Messages/Commit.cs
- Plugins/DBFTPlugin/Messages/ConsensusMessage.cs
- Plugins/DBFTPlugin/Messages/PrepareRequest.cs
- Plugins/DBFTPlugin/Messages/PrepareResponse.cs
- Plugins/DBFTPlugin/Messages/RecoveryMessage/RecoveryMessage.ChangeViewPayloadCompact.cs
- Plugins/DBFTPlugin/Messages/RecoveryMessage/RecoveryMessage.CommitPayloadCompact.cs
- Plugins/DBFTPlugin/Messages/RecoveryMessage/RecoveryMessage.PreparationPayloadCompact.cs
- Plugins/DBFTPlugin/Messages/RecoveryMessage/RecoveryMessage.cs
- Plugins/DBFTPlugin/Messages/RecoveryMessage/RecoveryRequest.cs
- Plugins/DBFTPlugin/Types/ChangeViewReason.cs
- Plugins/DBFTPlugin/Types/ConsensusMessageType.cs

### Neo/Network/P2P
- Neo/Network/P2P/Capabilities/ArchivalNodeCapability.cs
- Neo/Network/P2P/Capabilities/DisableCompressionCapability.cs
- Neo/Network/P2P/Capabilities/FullNodeCapability.cs
- Neo/Network/P2P/Capabilities/NodeCapability.cs
- Neo/Network/P2P/Capabilities/NodeCapabilityType.cs
- Neo/Network/P2P/Capabilities/ServerCapability.cs
- Neo/Network/P2P/Capabilities/UnknownCapability.cs
- Neo/Network/P2P/ChannelsConfig.cs
- Neo/Network/P2P/Helper.cs
- Neo/Network/P2P/LocalNode.cs
- Neo/Network/P2P/Message.cs
- Neo/Network/P2P/MessageCommand.cs
- Neo/Network/P2P/MessageFlags.cs
- Neo/Network/P2P/Peer.cs
- Neo/Network/P2P/RemoteNode.ProtocolHandler.cs
- Neo/Network/P2P/RemoteNode.cs
- Neo/Network/P2P/TaskManager.cs
- Neo/Network/P2P/TaskSession.cs

### Neo/Persistence
- Neo/Persistence/ClonedCache.cs
- Neo/Persistence/DataCache.cs
- Neo/Persistence/IReadOnlyStore.cs
- Neo/Persistence/IStore.cs
- Neo/Persistence/IStoreProvider.cs
- Neo/Persistence/IStoreSnapshot.cs
- Neo/Persistence/IWriteStore.cs
- Neo/Persistence/Providers/MemorySnapshot.cs
- Neo/Persistence/Providers/MemoryStore.cs
- Neo/Persistence/Providers/MemoryStoreProvider.cs
- Neo/Persistence/SeekDirection.cs
- Neo/Persistence/StoreCache.cs
- Neo/Persistence/StoreFactory.cs
- Neo/Persistence/TrackState.cs

### Neo/SmartContract/Native
- Neo/SmartContract/Native/AccountState.cs
- Neo/SmartContract/Native/ContractEventAttribute.cs
- Neo/SmartContract/Native/ContractMethodAttribute.cs
- Neo/SmartContract/Native/ContractMethodMetadata.cs
- Neo/SmartContract/Native/CryptoLib.BLS12_381.cs
- Neo/SmartContract/Native/HashIndexState.cs
- Neo/SmartContract/Native/IHardforkActivable.cs
- Neo/SmartContract/Native/InteroperableList.cs
- Neo/SmartContract/Native/NamedCurveHash.cs
- Neo/SmartContract/Native/Notary.cs
- Neo/SmartContract/Native/OracleRequest.cs
- Neo/SmartContract/Native/Role.cs
- Neo/SmartContract/Native/TransactionState.cs
- Neo/SmartContract/Native/TrimmedBlock.cs

### Plugins/ApplicationLogs
- Plugins/ApplicationLogs/LogReader.cs
- Plugins/ApplicationLogs/Settings.cs
- Plugins/ApplicationLogs/Store/LogStorageStore.cs
- Plugins/ApplicationLogs/Store/Models/ApplicationEngineLogModel.cs
- Plugins/ApplicationLogs/Store/Models/BlockchainEventModel.cs
- Plugins/ApplicationLogs/Store/Models/BlockchainExecutionModel.cs
- Plugins/ApplicationLogs/Store/NeoStore.cs
- Plugins/ApplicationLogs/Store/States/BlockLogState.cs
- Plugins/ApplicationLogs/Store/States/ContractLogState.cs
- Plugins/ApplicationLogs/Store/States/EngineLogState.cs
- Plugins/ApplicationLogs/Store/States/ExecutionLogState.cs
- Plugins/ApplicationLogs/Store/States/NotifyLogState.cs
- Plugins/ApplicationLogs/Store/States/TransactionEngineLogState.cs
- Plugins/ApplicationLogs/Store/States/TransactionLogState.cs

### Plugins/LevelDBStore
- Plugins/LevelDBStore/IO/Data/LevelDB/DB.cs
- Plugins/LevelDBStore/IO/Data/LevelDB/Helper.cs
- Plugins/LevelDBStore/IO/Data/LevelDB/Iterator.cs
- Plugins/LevelDBStore/IO/Data/LevelDB/LevelDBException.cs
- Plugins/LevelDBStore/IO/Data/LevelDB/LevelDBHandle.cs
- Plugins/LevelDBStore/IO/Data/LevelDB/Native.cs
- Plugins/LevelDBStore/IO/Data/LevelDB/Options.cs
- Plugins/LevelDBStore/IO/Data/LevelDB/ReadOptions.cs
- Plugins/LevelDBStore/IO/Data/LevelDB/Snapshot.cs
- Plugins/LevelDBStore/IO/Data/LevelDB/WriteBatch.cs
- Plugins/LevelDBStore/IO/Data/LevelDB/WriteOptions.cs
- Plugins/LevelDBStore/Plugins/Storage/LevelDBStore.cs
- Plugins/LevelDBStore/Plugins/Storage/Snapshot.cs
- Plugins/LevelDBStore/Plugins/Storage/Store.cs

### Neo.Json
- Neo.Json/GlobalSuppressions.cs
- Neo.Json/JArray.cs
- Neo.Json/JBoolean.cs
- Neo.Json/JContainer.cs
- Neo.Json/JNumber.cs
- Neo.Json/JObject.cs
- Neo.Json/JPathToken.cs
- Neo.Json/JPathTokenType.cs
- Neo.Json/JString.cs
- Neo.Json/JToken.cs
- Neo.Json/OrderedDictionary.KeyCollection.cs
- Neo.Json/OrderedDictionary.ValueCollection.cs

### Plugins/TokensTracker
- Plugins/TokensTracker/Extensions.cs
- Plugins/TokensTracker/TokensTracker.cs
- Plugins/TokensTracker/Trackers/NEP-11/Nep11BalanceKey.cs
- Plugins/TokensTracker/Trackers/NEP-11/Nep11Tracker.cs
- Plugins/TokensTracker/Trackers/NEP-11/Nep11TransferKey.cs
- Plugins/TokensTracker/Trackers/NEP-17/Nep17BalanceKey.cs
- Plugins/TokensTracker/Trackers/NEP-17/Nep17Tracker.cs
- Plugins/TokensTracker/Trackers/NEP-17/Nep17TransferKey.cs
- Plugins/TokensTracker/Trackers/TokenBalance.cs
- Plugins/TokensTracker/Trackers/TokenTransfer.cs
- Plugins/TokensTracker/Trackers/TokenTransferKey.cs
- Plugins/TokensTracker/Trackers/TrackerBase.cs

### Neo.Extensions
- Neo.Extensions/AssemblyExtensions.cs
- Neo.Extensions/BigIntegerExtensions.cs
- Neo.Extensions/ByteArrayComparer.cs
- Neo.Extensions/ByteArrayEqualityComparer.cs
- Neo.Extensions/ByteExtensions.cs
- Neo.Extensions/DateTimeExtensions.cs
- Neo.Extensions/IntegerExtensions.cs
- Neo.Extensions/LogLevel.cs
- Neo.Extensions/SecureStringExtensions.cs
- Neo.Extensions/StringExtensions.cs
- Neo.Extensions/Utility.cs

### Neo/Network/P2P/Payloads/Conditions
- Neo/Network/P2P/Payloads/Conditions/AndCondition.cs
- Neo/Network/P2P/Payloads/Conditions/BooleanCondition.cs
- Neo/Network/P2P/Payloads/Conditions/CalledByContractCondition.cs
- Neo/Network/P2P/Payloads/Conditions/CalledByEntryCondition.cs
- Neo/Network/P2P/Payloads/Conditions/CalledByGroupCondition.cs
- Neo/Network/P2P/Payloads/Conditions/GroupCondition.cs
- Neo/Network/P2P/Payloads/Conditions/NotCondition.cs
- Neo/Network/P2P/Payloads/Conditions/OrCondition.cs
- Neo/Network/P2P/Payloads/Conditions/ScriptHashCondition.cs
- Neo/Network/P2P/Payloads/Conditions/WitnessCondition.cs
- Neo/Network/P2P/Payloads/Conditions/WitnessConditionType.cs

### Neo/IEventHandlers
- Neo/IEventHandlers/ICommittedHandler.cs
- Neo/IEventHandlers/ICommittingHandler.cs
- Neo/IEventHandlers/ILogHandler.cs
- Neo/IEventHandlers/ILoggingHandler.cs
- Neo/IEventHandlers/IMessageReceivedHandler.cs
- Neo/IEventHandlers/INotifyHandler.cs
- Neo/IEventHandlers/IServiceAddedHandler.cs
- Neo/IEventHandlers/ITransactionAddedHandler.cs
- Neo/IEventHandlers/ITransactionRemovedHandler.cs
- Neo/IEventHandlers/IWalletChangedHandler.cs

### Neo/Wallets
- Neo/Wallets/AssetDescriptor.cs
- Neo/Wallets/Helper.cs
- Neo/Wallets/IWalletFactory.cs
- Neo/Wallets/IWalletProvider.cs
- Neo/Wallets/NEP6/NEP6Account.cs
- Neo/Wallets/NEP6/NEP6Contract.cs
- Neo/Wallets/NEP6/NEP6Wallet.cs
- Neo/Wallets/NEP6/NEP6WalletFactory.cs
- Neo/Wallets/NEP6/ScryptParameters.cs
- Neo/Wallets/TransferOutput.cs

### Plugins/StateService
- Plugins/StateService/Network/MessageType.cs
- Plugins/StateService/Network/StateRoot.cs
- Plugins/StateService/Network/Vote.cs
- Plugins/StateService/StatePlugin.cs
- Plugins/StateService/StateServiceSettings.cs
- Plugins/StateService/Storage/Keys.cs
- Plugins/StateService/Storage/StateSnapshot.cs
- Plugins/StateService/Storage/StateStore.cs
- Plugins/StateService/Verification/VerificationContext.cs
- Plugins/StateService/Verification/VerificationService.cs

### Neo.Cryptography.MPTTrie
- Neo.Cryptography.MPTTrie/Node.Branch.cs
- Neo.Cryptography.MPTTrie/Node.Extension.cs
- Neo.Cryptography.MPTTrie/Node.Hash.cs
- Neo.Cryptography.MPTTrie/Node.Leaf.cs
- Neo.Cryptography.MPTTrie/Trie.Delete.cs
- Neo.Cryptography.MPTTrie/Trie.Find.cs
- Neo.Cryptography.MPTTrie/Trie.Get.cs
- Neo.Cryptography.MPTTrie/Trie.Proof.cs
- Neo.Cryptography.MPTTrie/Trie.Put.cs

### Plugins/SQLiteWallet
- Plugins/SQLiteWallet/Account.cs
- Plugins/SQLiteWallet/Address.cs
- Plugins/SQLiteWallet/Contract.cs
- Plugins/SQLiteWallet/Key.cs
- Plugins/SQLiteWallet/SQLiteWallet.cs
- Plugins/SQLiteWallet/SQLiteWalletAccount.cs
- Plugins/SQLiteWallet/SQLiteWalletFactory.cs
- Plugins/SQLiteWallet/VerificationContract.cs
- Plugins/SQLiteWallet/WalletDataContext.cs

### Neo.ConsoleService
- Neo.ConsoleService/CommandToken.cs
- Neo.ConsoleService/CommandTokenizer.cs
- Neo.ConsoleService/ConsoleColorSet.cs
- Neo.ConsoleService/ConsoleCommandAttribute.cs
- Neo.ConsoleService/ConsoleCommandMethod.cs
- Neo.ConsoleService/ConsoleHelper.cs
- Neo.ConsoleService/ConsoleServiceBase.cs
- Neo.ConsoleService/ServiceProxy.cs

### Neo/Ledger
- Neo/Ledger/Blockchain.ApplicationExecuted.cs
- Neo/Ledger/Blockchain.cs
- Neo/Ledger/MemoryPool.cs
- Neo/Ledger/PoolItem.cs
- Neo/Ledger/TransactionRemovalReason.cs
- Neo/Ledger/TransactionRemovedEventArgs.cs
- Neo/Ledger/TransactionRouter.cs
- Neo/Ledger/TransactionVerificationContext.cs

### Neo
- Neo/ContainsTransactionType.cs
- Neo/Plugins/IPluginSettings.cs
- Neo/Plugins/Plugin.cs
- Neo/Plugins/UnhandledExceptionPolicy.cs
- Neo/Properties/AssemblyInfo.cs
- Neo/ProtocolSettings.cs
- Neo/TimeProvider.cs

### Neo/SmartContract/ApplicationEngine
- Neo/SmartContract/ApplicationEngine.Contract.cs
- Neo/SmartContract/ApplicationEngine.Crypto.cs
- Neo/SmartContract/ApplicationEngine.Helper.cs
- Neo/SmartContract/ApplicationEngine.Iterator.cs
- Neo/SmartContract/ApplicationEngine.OpCodePrices.cs
- Neo/SmartContract/ApplicationEngine.cs

### Plugins/OracleService
- Plugins/OracleService/Helper.cs
- Plugins/OracleService/OracleService.cs
- Plugins/OracleService/OracleSettings.cs
- Plugins/OracleService/Protocols/IOracleProtocol.cs
- Plugins/OracleService/Protocols/OracleHttpsProtocol.cs
- Plugins/OracleService/Protocols/OracleNeoFSProtocol.cs

### Neo/Builders
- Neo/Builders/AndConditionBuilder.cs
- Neo/Builders/OrConditionBuilder.cs
- Neo/Builders/TransactionAttributesBuilder.cs
- Neo/Builders/WitnessConditionBuilder.cs
- Neo/Builders/WitnessRuleBuilder.cs

### Neo/Extensions
- Neo/Extensions/ByteExtensions.cs
- Neo/Extensions/MemoryExtensions.cs
- Neo/Extensions/NeoSystemExtensions.cs
- Neo/Extensions/SpanExtensions.cs
- Neo/Extensions/UInt160Extensions.cs

### Neo/SmartContract/Manifest
- Neo/SmartContract/Manifest/ContractEventDescriptor.cs
- Neo/SmartContract/Manifest/ContractMethodDescriptor.cs
- Neo/SmartContract/Manifest/ContractParameterDefinition.cs
- Neo/SmartContract/Manifest/ContractPermissionDescriptor.cs
- Neo/SmartContract/Manifest/WildCardContainer.cs

### Neo.IO/Caching
- Neo.IO/Caching/HashSetCache.cs
- Neo.IO/Caching/IndexedQueue.cs
- Neo.IO/Caching/KeyedCollectionSlim.cs
- Neo.IO/Caching/ReflectionCacheAttribute.cs

### Neo/Cryptography
- Neo/Cryptography/MerkleTreeNode.cs
- Neo/Cryptography/Murmur128.cs
- Neo/Cryptography/Murmur32.cs
- Neo/Cryptography/RIPEMD160Managed.cs

### Neo/Extensions/IO
- Neo/Extensions/IO/BinaryReaderExtensions.cs
- Neo/Extensions/IO/BinaryWriterExtensions.cs
- Neo/Extensions/IO/ISerializableExtensions.cs
- Neo/Extensions/IO/MemoryReaderExtensions.cs

### Neo/Extensions/SmartContract
- Neo/Extensions/SmartContract/ContractParameterExtensions.cs
- Neo/Extensions/SmartContract/ContractStateExtensions.cs
- Neo/Extensions/SmartContract/GasTokenExtensions.cs
- Neo/Extensions/SmartContract/NeoTokenExtensions.cs

### Plugins/RocksDBStore
- Plugins/RocksDBStore/Plugins/Storage/Options.cs
- Plugins/RocksDBStore/Plugins/Storage/RocksDBStore.cs
- Plugins/RocksDBStore/Plugins/Storage/Snapshot.cs
- Plugins/RocksDBStore/Plugins/Storage/Store.cs

### Neo/Cryptography/ECC
- Neo/Cryptography/ECC/ECCurve.cs
- Neo/Cryptography/ECC/ECFieldElement.cs
- Neo/Cryptography/ECC/ECPoint.cs

### Neo/Extensions/VM
- Neo/Extensions/VM/EvaluationStackExtensions.cs
- Neo/Extensions/VM/ScriptBuilderExtensions.cs
- Neo/Extensions/VM/StackItemExtensions.cs

### Neo/Sign
- Neo/Sign/ISigner.cs
- Neo/Sign/SignException.cs
- Neo/Sign/SignerManager.cs

### Plugins/SignClient
- Plugins/SignClient/SignClient.cs
- Plugins/SignClient/SignSettings.cs
- Plugins/SignClient/Vsock.cs

### Neo.Extensions/Collections
- Neo.Extensions/Collections/CollectionExtensions.cs
- Neo.Extensions/Collections/HashSetExtensions.cs

### Neo.IO
- Neo.IO/ISerializable.cs
- Neo.IO/ISerializableSpan.cs

### Neo/IO/Caching
- Neo/IO/Caching/ECDsaCache.cs
- Neo/IO/Caching/ECPointCache.cs

### Neo/SmartContract/Interop
- Neo/SmartContract/InteropDescriptor.cs
- Neo/SmartContract/InteropParameterDescriptor.cs

### Neo/SmartContract/Iterators
- Neo/SmartContract/Iterators/IIterator.cs
- Neo/SmartContract/Iterators/StorageIterator.cs

### Plugins/StorageDumper
- Plugins/StorageDumper/StorageDumper.cs
- Plugins/StorageDumper/StorageSettings.cs

### Neo.Extensions/Exceptions
- Neo.Extensions/Exceptions/TryCatchExtensions.cs

### Neo.Extensions/Factories
- Neo.Extensions/Factories/RandomNumberFactory.cs

### Neo.Extensions/Net
- Neo.Extensions/Net/IpAddressExtensions.cs

### Neo.IO/Actors
- Neo.IO/Actors/Idle.cs

### Neo/Extensions/Collections
- Neo/Extensions/Collections/ICollectionExtensions.cs

### Neo/Network
- Neo/Network/UPnP.cs

### Neo/SmartContract/Json
- Neo/SmartContract/JsonSerializer.cs

