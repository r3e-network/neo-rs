# Plugin Parity Audit

Total missing plugin files: 208

## DBFTPlugin
Missing files: 20

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

## RpcServer
Missing files: 22

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

## RestServer
Missing files: 92

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

## OracleService
Missing files: 6

- Plugins/OracleService/Helper.cs
- Plugins/OracleService/OracleService.cs
- Plugins/OracleService/OracleSettings.cs
- Plugins/OracleService/Protocols/IOracleProtocol.cs
- Plugins/OracleService/Protocols/OracleHttpsProtocol.cs
- Plugins/OracleService/Protocols/OracleNeoFSProtocol.cs

## TokensTracker
Missing files: 12

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

## SQLiteWallet
Missing files: 9

- Plugins/SQLiteWallet/Account.cs
- Plugins/SQLiteWallet/Address.cs
- Plugins/SQLiteWallet/Contract.cs
- Plugins/SQLiteWallet/Key.cs
- Plugins/SQLiteWallet/SQLiteWallet.cs
- Plugins/SQLiteWallet/SQLiteWalletAccount.cs
- Plugins/SQLiteWallet/SQLiteWalletFactory.cs
- Plugins/SQLiteWallet/VerificationContract.cs
- Plugins/SQLiteWallet/WalletDataContext.cs

## StateService
Missing files: 10

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

## ApplicationLogs
Missing files: 14

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

## StorageDumper
Missing files: 2

- Plugins/StorageDumper/StorageDumper.cs
- Plugins/StorageDumper/StorageSettings.cs

## SignClient
Missing files: 3

- Plugins/SignClient/SignClient.cs
- Plugins/SignClient/SignSettings.cs
- Plugins/SignClient/Vsock.cs

## LevelDBStore
Missing files: 14

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

## RocksDBStore
Missing files: 4

- Plugins/RocksDBStore/Plugins/Storage/Options.cs
- Plugins/RocksDBStore/Plugins/Storage/RocksDBStore.cs
- Plugins/RocksDBStore/Plugins/Storage/Snapshot.cs
- Plugins/RocksDBStore/Plugins/Storage/Store.cs

