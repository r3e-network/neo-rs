# Tooling & Auxiliary Modules Parity Audit

Total missing tooling files: 154

## CLI
Missing files: 22

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

## ConsoleService
Missing files: 8

- Neo.ConsoleService/CommandToken.cs
- Neo.ConsoleService/CommandTokenizer.cs
- Neo.ConsoleService/ConsoleColorSet.cs
- Neo.ConsoleService/ConsoleCommandAttribute.cs
- Neo.ConsoleService/ConsoleCommandMethod.cs
- Neo.ConsoleService/ConsoleHelper.cs
- Neo.ConsoleService/ConsoleServiceBase.cs
- Neo.ConsoleService/ServiceProxy.cs

## RpcClient
Missing files: 35

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

## BLS12_381
Missing files: 26

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

## MPTTrie
Missing files: 9

- Neo.Cryptography.MPTTrie/Node.Branch.cs
- Neo.Cryptography.MPTTrie/Node.Extension.cs
- Neo.Cryptography.MPTTrie/Node.Hash.cs
- Neo.Cryptography.MPTTrie/Node.Leaf.cs
- Neo.Cryptography.MPTTrie/Trie.Delete.cs
- Neo.Cryptography.MPTTrie/Trie.Find.cs
- Neo.Cryptography.MPTTrie/Trie.Get.cs
- Neo.Cryptography.MPTTrie/Trie.Proof.cs
- Neo.Cryptography.MPTTrie/Trie.Put.cs

## JSON
Missing files: 12

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

## Extensions
Missing files: 33

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
- Neo.Extensions/Collections/CollectionExtensions.cs
- Neo.Extensions/Collections/HashSetExtensions.cs
- Neo.Extensions/Exceptions/TryCatchExtensions.cs
- Neo.Extensions/Factories/RandomNumberFactory.cs
- Neo.Extensions/Net/IpAddressExtensions.cs
- Neo/Extensions/ByteExtensions.cs
- Neo/Extensions/MemoryExtensions.cs
- Neo/Extensions/NeoSystemExtensions.cs
- Neo/Extensions/SpanExtensions.cs
- Neo/Extensions/UInt160Extensions.cs
- Neo/Extensions/Collections/ICollectionExtensions.cs
- Neo/Extensions/IO/BinaryReaderExtensions.cs
- Neo/Extensions/IO/BinaryWriterExtensions.cs
- Neo/Extensions/IO/ISerializableExtensions.cs
- Neo/Extensions/IO/MemoryReaderExtensions.cs
- Neo/Extensions/SmartContract/ContractParameterExtensions.cs
- Neo/Extensions/SmartContract/ContractStateExtensions.cs
- Neo/Extensions/SmartContract/GasTokenExtensions.cs
- Neo/Extensions/SmartContract/NeoTokenExtensions.cs
- Neo/Extensions/VM/EvaluationStackExtensions.cs
- Neo/Extensions/VM/ScriptBuilderExtensions.cs
- Neo/Extensions/VM/StackItemExtensions.cs

## IO
Missing files: 9

- Neo.IO/ISerializable.cs
- Neo.IO/ISerializableSpan.cs
- Neo.IO/Actors/Idle.cs
- Neo.IO/Caching/HashSetCache.cs
- Neo.IO/Caching/IndexedQueue.cs
- Neo.IO/Caching/KeyedCollectionSlim.cs
- Neo.IO/Caching/ReflectionCacheAttribute.cs
- Neo/IO/Caching/ECDsaCache.cs
- Neo/IO/Caching/ECPointCache.cs

