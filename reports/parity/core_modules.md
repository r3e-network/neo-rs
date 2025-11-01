# Core Module Parity Audit

Total missing files across core modules: 219

## Core
Missing files: 31

- Neo/ContainsTransactionType.cs
- Neo/Plugins/IPluginSettings.cs
- Neo/Plugins/Plugin.cs
- Neo/Plugins/UnhandledExceptionPolicy.cs
- Neo/Properties/AssemblyInfo.cs
- Neo/ProtocolSettings.cs
- Neo/TimeProvider.cs
- Neo/Builders/AndConditionBuilder.cs
- Neo/Builders/OrConditionBuilder.cs
- Neo/Builders/TransactionAttributesBuilder.cs
- Neo/Builders/WitnessConditionBuilder.cs
- Neo/Builders/WitnessRuleBuilder.cs
- Neo/Sign/ISigner.cs
- Neo/Sign/SignException.cs
- Neo/Sign/SignerManager.cs
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
- Neo.IO/Caching/HashSetCache.cs
- Neo.IO/Caching/IndexedQueue.cs
- Neo.IO/Caching/KeyedCollectionSlim.cs
- Neo.IO/Caching/ReflectionCacheAttribute.cs
- Neo/IO/Caching/ECDsaCache.cs
- Neo/IO/Caching/ECPointCache.cs

## SmartContract
Missing files: 59

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
- Neo/SmartContract/ApplicationEngine.Contract.cs
- Neo/SmartContract/ApplicationEngine.Crypto.cs
- Neo/SmartContract/ApplicationEngine.Helper.cs
- Neo/SmartContract/ApplicationEngine.Iterator.cs
- Neo/SmartContract/ApplicationEngine.OpCodePrices.cs
- Neo/SmartContract/ApplicationEngine.cs
- Neo/SmartContract/InteropDescriptor.cs
- Neo/SmartContract/InteropParameterDescriptor.cs
- Neo/SmartContract/Iterators/IIterator.cs
- Neo/SmartContract/Iterators/StorageIterator.cs
- Neo/SmartContract/JsonSerializer.cs
- Neo/SmartContract/Manifest/ContractEventDescriptor.cs
- Neo/SmartContract/Manifest/ContractMethodDescriptor.cs
- Neo/SmartContract/Manifest/ContractParameterDefinition.cs
- Neo/SmartContract/Manifest/ContractPermissionDescriptor.cs
- Neo/SmartContract/Manifest/WildCardContainer.cs
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

## VM
Missing files: 39

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

## Ledger
Missing files: 8

- Neo/Ledger/Blockchain.ApplicationExecuted.cs
- Neo/Ledger/Blockchain.cs
- Neo/Ledger/MemoryPool.cs
- Neo/Ledger/PoolItem.cs
- Neo/Ledger/TransactionRemovalReason.cs
- Neo/Ledger/TransactionRemovedEventArgs.cs
- Neo/Ledger/TransactionRouter.cs
- Neo/Ledger/TransactionVerificationContext.cs

## Network
Missing files: 58

- Neo/Network/UPnP.cs
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

## Persistence
Missing files: 14

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

## Wallets
Missing files: 10

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

