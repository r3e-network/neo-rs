# neo-rpc JSON-RPC 协议合规性审计（第二轮 · RPC 层）

- 审计对象：`d:\Git\neo-rs\neo-rpc\src\`（及其依赖的 `neo-core` / `neo-primitives` 中与 RPC 输出相关的 JSON 序列化代码）
- 基线：C# `neo-project/neo` **v3.10.1** + `neo-project/neo-modules`（RpcServer / StateService / TokensTracker / ApplicationLogs / OracleService）
- 方法：静态分析。C# 行为一律以 `raw.githubusercontent.com` 抓取到的源码为准，不凭记忆断言。
- 未修改任何代码；未执行任何 `cargo` 命令。
- 与首轮（`docs/audits/2026-08-28-protocol-compliance-audit.md`，F1–F10）无重叠：首轮未覆盖 `neo-rpc`。

## 基线来源与局限

| 仓库 | 使用的 ref | 说明 |
|---|---|---|
| `neo-project/neo` | tag `v3.10.1` | 存在。用于 `Signer/Header/ContractState/NefFile/ApplicationEngine/ProtocolSettings/NativeContract/WitnessScope/TrackState/Wallet` |
| `neo-project/neo-modules` | `master` | **该仓库的 tag 最高只到 `v3.7.5`**，`v3.10.1`/`v3.10.0`/`v3.9.1` 均 404（已用 `api.github.com` 校验 tag 列表）。RpcServer / StateService / TokensTracker / ApplicationLogs / OracleService 全部取自 `master`，并用 `v3.7.5` 的 `RpcError.cs`、`RpcServer.Blockchain.cs` 交叉校验（错误码与方法集合完全一致，`master` 仅多出 `getnativecontracts`） |

---

## 一、方法清单与命名

**结论：Rust 覆盖了 C# 的全部 50 个 RPC 方法，无遗漏、无拼写/大小写偏差。**（`resolve_rpc_handler` 对 method 做 `to_ascii_lowercase`，与 C# `method.Name.ToLowerInvariant()` 一致。）

C# 方法全集（从源码枚举，共 50 个）：

- `RpcServer.Blockchain.cs`（16）：getbestblockhash、getblock、getblockheadercount、getblockcount、getblockhash、getblockheader、getcontractstate、getrawmempool、getrawtransaction、getstorage、findstorage、gettransactionheight、getnextblockvalidators、getcandidates、getcommittee、getnativecontracts
- `RpcServer.Node.cs`（5）：getconnectioncount、getpeers、getversion、sendrawtransaction、submitblock
- `RpcServer.SmartContract.cs`（5）：invokefunction、invokescript、traverseiterator、terminatesession、getunclaimedgas
- `RpcServer.Utilities.cs`（2）：listplugins、validateaddress
- `RpcServer.Wallet.cs`（14）：closewallet、dumpprivkey、getnewaddress、getwalletbalance、getwalletunclaimedgas、importprivkey、calculatenetworkfee、listaddress、openwallet、sendfrom、sendmany、sendtoaddress、canceltransaction、invokecontractverify
- `StateService/StatePlugin.cs`（6）：getstateroot、getproof、verifyproof、getstateheight、findstates、getstate
- `ApplicationLogs/LogReader.cs`（1）：getapplicationlog
- `OracleService/OracleService.cs`（1）：submitoracleresponse
- `TokensTracker`（5）：getnep17balances、getnep17transfers、getnep11balances、getnep11transfers、getnep11properties

Rust 侧额外注册了 4 个非 C# 方法：`legacy`、`ping`、`subscribe`、`unsubscribe`（扩展，不属于偏差）。

> 任务书中点名要求核对的 `getproofroot`、`getproofstate`、`getmempooldump`：经抓取 C# `StatePlugin.cs`（方法仅 `GetStateRoot/GetProof/VerifyProof/GetStateHeight/FindStates/GetState`）与 `RpcServer.Blockchain.cs` 确认，**C# v3 主线不存在这三个方法**。Rust 未实现它们是正确的。

---

## 二、Findings

| 编号 | 严重度 | 文件:行 | 问题 | C# 参考（证据 + 抓取到的源码要点） | 建议修复 |
|---|---|---|---|---|---|
| R2-01 | CRITICAL | `neo-rpc/src/server/rpc_error.rs:189` | `invalid_script` 错误码为 **-506** | C# `RpcError.cs:58`：`InvalidScript = new(-509, "Invalid transaction script")`。C# 中 **-506 不存在**（`-505 PolicyFailed` 之后直接是 `-507 InvalidAttribute`）。同一文件 `RpcError.cs:61` 另有 `InvalidSize = new(-509, "Invalid inventory size")`，即 **C# 上游本身存在 -509 重复**（`UT_RpcError.cs` 的 `AllDifferent` 测试基于 `ToString()` 而非 `Code`，未捕获该重复），属于必须照抄的上游 bug | 改为 `-509`；保留 `invalid_size` 亦为 `-509`（与 C# 一致地"重复"）。注意下游客户端常按 code 分支，改对后 `invalid_script` 与 `invalid_size` 不可区分，这是 C# 的既定行为 |
| R2-02 | CRITICAL | `neo-rpc/src/server/rpc_error.rs:172` | 自造错误码 **-305 `unknown_account`（"Unknown account"）** | C# `RpcError.cs` 中 -304 之后即 -500，**不存在 -305**（完整表见下节）。若某钱包/账户场景需要报错，C# 会走 `-32602 InvalidParams` 或 `-300 InsufficientFundsWallet` | 删除该错误码；调用点改用它处已有的 `-32602 invalid_params(...).with_data(...)` 或按需用 `-300` |
| R2-03 | CRITICAL | `neo-rpc/src/server/rpc_server_state.rs:86-91` | `verifyproof` 校验失败返回 **-500 `verification_failed`（"Inventory verification failed"）** | C# `StatePlugin.cs:247`：`var value = Trie.VerifyProof(root_hash, key, proofs).NotNull_Or(RpcError.InvalidProof);`；`RpcError.cs:73`：`InvalidProof = new(-607, "Invalid state proof")` | Rust 已定义 `invalid_proof => (-607, "Invalid state proof")`（`rpc_error.rs:219`）但未被使用。将 `rpc_server_state.rs:88` 的 `RpcError::verification_failed()` 改为 `RpcError::invalid_proof()`（data 可保留或去掉；C# 此处无 data） |
| R2-04 | HIGH | `neo-rpc/src/server/smart_contract/helpers.rs:176-181` | 迭代器型 `InteropInterface` 输出 `"interface": "StorageIterator"` | C# `RpcServer.SmartContract.cs:163-168`：`if (item is InteropInterface interopInterface && interopInterface.GetInterface<object>() is IIterator iterator) { ... json["interface"] = nameof(IIterator); json["id"] = id.ToString(); }` → 实际值为 `"IIterator"` | 改为 `Value::String("IIterator".to_string())` |
| R2-05 | HIGH | `neo-primitives/src/witness_scope.rs:242` | 多标志组合用 `" \| "` 连接，输出如 `"CalledByEntry \| CustomContracts"` | C# `Signer.ToJson()` 为 `json["scopes"] = Scopes;`（枚举对象直接进 `JObject`，序列化取 `Enum.ToString()`）。官方文档 `docs.neo.org` 的 `getrawtransaction` 实测样例确认为字符串形式（`"scopes": "CalledByEntry"` / `"scopes": "None"` / `"FeeOnly"`）；.NET `[Flags]` 枚举在组合值时 `ToString()` 以 **逗号+空格 `", "`** 连接 | `parts.join(" | ")` 改为 `parts.join(", ")`。同时确认 `NAMED_FLAGS` 的枚举次序与 C# 值顺序（None/CalledByEntry/CustomContracts/CustomGroups/WitnessRules/Global）一致 |
| R2-06 | HIGH | `neo-core/src/network/p2p/payloads/signer.rs:134、147、159` | 仅当 scope 标志置位**且数组非空**时才输出 `allowedcontracts` / `allowedgroups` / `rules` | C# `Signer.ToJson()`：`if (Scopes.HasFlag(WitnessScope.CustomContracts)) { json["allowedcontracts"] = new JArray(AllowedContracts.Select(...)); }` —— **只看标志位，空数组也会输出 `[]`** | 去掉 `&& !self.allowed_contracts.is_empty()` / `!self.allowed_groups.is_empty()` / `!self.rules.is_empty()` 三个条件，改为纯标志位判断 |
| R2-07 | MEDIUM | `neo-rpc/src/server/rpc_server_node/mod.rs:159-165` | `getversion` 额外输出 `protocol.standbycommittee`、`protocol.seedlist` 两个 C# 没有的字段 | C# `RpcServer.Node.cs:113-145`：顶层仅 `tcpport / nonce / useragent / rpc{maxiteratorresultitems, sessionenabled} / protocol{addressversion, network, validatorscount, msperblock, maxtraceableblocks, maxvaliduntilblockincrement, maxtransactionsperblock, memorypoolmaxtransactions, initialgasdistribution, hardforks[{name, blockheight}]}`。**C# 也没有 `magic` 和 `wsport`**（Rust 未输出这两项是正确的） | 删除这两个字段，或置于显式的"非标准扩展"开关后。严格 schema 的 C# 客户端（如 NeoGo 的兼容校验）会因此报错 |
| R2-08 | MEDIUM | `neo-rpc/src/server/rpc_server_blockchain/mod.rs:262-269` | `getrawtransaction`（verbose）额外输出 `vmstate` 字段 | C# `RpcServer.Blockchain.cs:171-178`：`json = Utility.TransactionToJson(tx, settings); if (state is not null) { json["blockhash"]=...; json["confirmations"]=...; json["blocktime"]=...; }` —— **verbose 下只追加 blockhash/confirmations/blocktime 三项** | 删除 `vmstate` 分支（`state.vm_state()` 相关代码一并移除） |
| R2-09 | MEDIUM | `neo-rpc/src/server/rpc_error.rs:162` | `wallet_fee_limit` 的 data 文本为 `...more than the MaxFee...Please increase your MaxFee value.` | C# `RpcError.cs:47`：`new(-301, "Wallet fee limit exceeded", "The necessary fee is more than the Max_fee, this transaction is failed. Please increase your Max_fee value.")` —— 是 **`Max_fee`**（下划线），不是 `MaxFee` | 改为 `Max_fee` 两处 |
| R2-10 | MEDIUM | `neo-rpc/src/server/rpc_error.rs:136` | 自造 **-32001 `too_many_requests`（"Too many requests"）** | C# RpcServer 无限流机制，`RpcError.cs` 中 -32000..-32099 段**完全未使用**；C# 被限流时行为未定义（warp/ASP.NET 层直接 429） | 属主动加固，建议保留但文档化；若追求严格一致，可回退到 HTTP 429 + `-32603`。代码注释已说明取 -32000..-32099，可用 |
| R2-11 | MEDIUM | `neo-core/src/protocol_settings.rs:183`（`default_settings()`） | 默认设置下 `hardforks: HashMap::new()` 为空 → `getversion` 输出 `"hardforks": []`，且 `is_hardfork_enabled()` 对所有分叉返回 `false` | C# `ProtocolSettings.cs:131`：`Hardforks = EnsureOmmitedHardforks(new Dictionary<Hardfork, uint>()).ToImmutableDictionary()` → **默认即 8 项、blockheight 全 0**（即全部分叉在高度 0 生效）。Rust 的 `ensure_omitted_hardforks`（`protocol_settings.rs:275-288`）逻辑与 C# `EnsureOmmitedHardforks`（`ProtocolSettings.cs:238-252`：按 `AllHardforks` 顺序填 0，遇到已配置项即 `break`）**完全一致**，问题只在于 `default_settings()` 未调用它 | 在 `default_settings()` 中对 `HashMap::new()` 调用一次 `ensure_omitted_hardforks`（对齐 C# `Default`）。该差异同时影响共识路径（`is_hardfork_enabled`），优先级高于纯 RPC 展示问题 |
| R2-12 | MEDIUM | `neo-rpc/src/server/rpc_server_tokens_tracker/mod.rs:142-149`（NEP-11）、`344-352`（NEP-17） | 时间窗默认值用 **哨兵值 0** 判断：`start_time == 0 → now-7d`、`end_time == 0 → now` | C# `Nep17Tracker.cs:149-152`：`ulong startTime = _params.Count > 1 ? (ulong)_params[1].AsNumber() : (DateTime.UtcNow - TimeSpan.FromDays(7)).ToTimestampMS(); ulong endTime = _params.Count > 2 ? (ulong)_params[2].AsNumber() : DateTime.UtcNow.ToTimestampMS();` —— **按参数"是否存在"判断**。显式传 `0` 时 C# 视为 epoch 0 | 改为按 `params.len() > N` 判断；`0` 应被当作合法时间戳 |
| R2-13 | MEDIUM | `neo-rpc/src/server/rpc_server_tokens_tracker/helpers.rs:33-53`（`parse_address_param`） | 先尝试 `UInt160::try_parse`，失败再回落 `to_script_hash`（地址解析） | C# `TokensTracker` 的 `GetScriptHashFromParam(string addressOrScriptHash)`：`return addressOrScriptHash.Length < 40 ? addressOrScriptHash.ToScriptHash(settings.AddressVersion) : UInt160.Parse(addressOrScriptHash);` —— **按字符串长度 40 分派**，与"能否被解析为 UInt160"无关 | 改为长度分派（`len < 40 → to_script_hash`，否则 `UInt160::try_parse`），与 C# 保持逐字符一致 |
| R2-14 | MEDIUM | `neo-rpc/src/server/smart_contract/contract_verify.rs:53` | `get_method_ref("verify", parameters.len())` 要求 **精确 arity** 匹配 | C# `RpcServer.Wallet.cs:358`：`var md = contract.Manifest.Abi.GetMethod("verify", -1).NotNull_Or(RpcErrorFactory.InvalidContractVerification(contract.Hash));` —— 用 **`-1` 表示任意参数个数** | 改为 `-1`（任意 arity）语义；或先按 `pcount` 查找、失败再按 `-1` 查找，但这会与 C# 在"声明了两个不同 arity 的 verify"时的选择不同，建议直接对齐 `-1` |
| R2-15 | LOW | `neo-rpc/src/server/routes/handlers.rs:294` | `method_not_found` 的 data 为裸方法名 | C# `RpcErrorFactory.cs`：`MethodNotFound(string method) => RpcError.MethodNotFound.WithData($"The method '{method}' doesn't exists.");` | 改为 `format!("The method '{method}' doesn't exists.")`（含原文的语法错误 `doesn't exists`，须照抄） |
| R2-16 | LOW | `neo-rpc/src/server/routes/handlers.rs:169-180` | 批量数组超过 `max_batch_size` 时返回自定义 `-32600 "Batch too large: N entries exceeds maximum of M"` | C# `RpcServer.ProcessRequestAsync` 对批量无条数上限（每个元素逐个 `ProcessAsync` 后组装数组） | 属运维保护，建议保留；仅需在文档中标注为 Rust 扩展 |
| R2-17 | LOW | `neo-rpc/src/server/smart_contract/helpers.rs:42-47` | `final_rpc_vm_state_string` 对 `NONE` / `BREAK` 返回 `internal_error("{state:?} is not a final VM state")` | C# `RpcServer.SmartContract.cs:77` `json["state"] = session.Engine.State;` 直接写枚举名（引擎终止态只会是 `HALT`/`FAULT`，但 C# 不做防御） | 补充 `NONE`/`BREAK` → 字符串映射，避免理论上把可表达状态变成 -32603 |
| R2-18 | LOW | `neo-rpc/src/server/rpc_server_utilities.rs:33-100` | `listplugins` 返回**合成的**插件清单（`RpcServer` + 按服务是否注册追加 `ApplicationLogs`/`StateService`/`TokensTracker` + 存储插件），并**按 name 排序**，版本号取自 `CARGO_PKG_VERSION` 或 `NEO_PLUGIN_VERSION` 环境变量 | C# `RpcServer.Utilities.cs` `ListPlugins` 返回 `Plugin.Plugins` 的真实列表（RpcServer 插件自身也在其中），每项 `name/version/interfaces`，**不排序** | 已知占位实现，差异可接受；建议在响应或文档中标注。若要做一致性测试，需能注入固定列表 |
| R2-19 | LOW | `neo-rpc/src/server/rpc_server_blockchain/mod.rs:107` | `getblockhash` 在 `height <= current` 但按索引查不到 hash 时返回 `-101 unknown_block` | C# `RpcServer.Blockchain.cs:74-82`：`if (height <= CurrentIndex) { return Ledger.GetBlockHash(snapshot, height).ToString(); } throw new RpcException(RpcError.UnknownHeight);` —— 未做 null 检查，缺失时会 `NullReferenceException` → 被 `ProcessRequestAsync` 兜底为 `NewCustomError(ex.HResult, ex.Message)` | Rust 行为更健壮，建议保留；仅在"必须复现 C# 异常码"的极端兼容目标下才需改 |
| R2-20 | LOW | `neo-rpc/src/server/rpc_server_tokens_tracker/mod.rs:148-150`、`350-352` | `end < start` 时返回 `invalid_params("endTime must be >= startTime")`（**带 data**） | C# `Nep17Tracker.cs:154`：`(endTime >= startTime).True_Or(RpcError.InvalidParams);` —— **不带 data** | 去掉 `.with_data(...)`，直接返回 `RpcError::invalid_params()` |
| R2-21 | LOW | `neo-rpc/src/server/rpc_error_factory.rs` | 提供 `invalid_contract_verification_hash(hash, pcount)`，data 为 `"The smart contract {hash} haven't got verify method with {pcount} input parameters."` | C# `RpcErrorFactory.cs`：`InvalidContractVerification(UInt160 contractHash) => RpcError.InvalidContractVerification.WithData($"The smart contract {contractHash} haven't got verify method.");` —— **无 pcount 变体**；另有 `InvalidContractVerification(string data)` 供任意 data | 与 R2-14 一并处理：若对齐 C# 的 `-1` 查找，则该变体可删除 |
| R2-22 | LOW | `neo-rpc/src/server/rpc_server_blockchain/mod.rs:481` | `getcandidates` 的 `active` 用 ECPoint **精确相等**判断 | C# `RpcServer.Blockchain.cs:307`：`json["active"] = validators.ToByteArray().ToHexString().Contains(publickey);` —— 对序列化后的字节数组做**子串包含**判断（33 字节定长对齐，实践中结果一致，但理论上存在跨边界误判） | 现状更正确，建议保留并注释说明差异 |

---

## 三、错误码逐项对照

C# `neo-modules/src/RpcServer/RpcError.cs`（master，与 v3.7.5 逐行一致）完整数值表：

```
-32600 Invalid request          -32601 Method not found      -32602 Invalid params
-32603 Internal server RpcError -32700 Bad request
-101 Unknown block              -102 Unknown contract        -103 Unknown transaction
-104 Unknown storage item       -105 Unknown script container -106 Unknown state root
-107 Unknown session            -108 Unknown iterator        -109 Unknown height
-300 Insufficient funds in wallet
-301 Wallet fee limit exceeded  (data: "...more than the Max_fee...Max_fee value.")
-302 No opened wallet           -303 Wallet not found        -304 Wallet not supported
-500 Inventory verification failed  -501 Inventory already exists
-502 Memory pool capacity reached  -503 Already in pool
-504 Insufficient network fee   -505 Policy check failed
-507 Invalid transaction attribute  -508 Invalid signature
-509 Invalid transaction script  -509 Invalid inventory size   ← 上游重复，需照抄
-510 Expired transaction        -511 Insufficient funds for fee
-512 Invalid contract verification function
-600 Access denied              -601 State iterator sessions disabled
-602 Oracle service disabled    -603 Oracle request already finished
-604 Oracle request not found   -605 Not a designated oracle node
-606 Old state not supported    -607 Invalid state proof     -608 Contract execution failed
```

Rust `rpc_error.rs:124-220` 与之逐项对比：**除 R2-01（-506 vs -509）、R2-02（多出 -305）、R2-10（多出 -32001）三条外，其余 code 与 message 全部一致**；`wallet_fee_limit` 的 data 文本见 R2-09。

`RpcError` 的 JSON 序列化行为：

- C# `RpcError.cs:91-101`：`ToJson()` → `json["code"]=Code; json["message"]=ErrorMessage; if (!string.IsNullOrEmpty(Data)) json["data"]=Data;`，其中 `ErrorMessage => string.IsNullOrEmpty(Data) ? Message : $"{Message} - {Data}"`
- Rust `rpc_error.rs:108-122`：`to_json()` → `code`（number）、`message = self.error_message()`（`"{message} - {data}"`，无 data 时仅 message）、`data` 仅在 `Some` 时输出
- **结论：完全对齐。** 权威证据：`UT_RpcError.cs` 断言 `RpcError.AccessDenied.ToJson().ToString(false) == "{\"code\":-600,\"message\":\"Access denied\"}"`，Rust 侧 `rpc_error.rs:242-251` 有等价单测

---

## 四、JSON 输入解析

| 项 | C# 行为（证据） | Rust 现状 | 结论 |
|---|---|---|---|
| `getblock` 第 2 参 verbose | `RpcServer.Blockchain.cs:~` `bool verbose = _params.Count >= 2 && _params[1].AsBoolean();`（缺省 false） | `parse_verbose(params.get(1))` | 一致 |
| `getrawtransaction` verbose | `bool verbose = _params.Count >= 2 && _params[1].AsBoolean();`（`Blockchain.cs:163`） | 同上 | 一致 |
| 嵌套深度 | `RpcServer.cs` `MaxParamsDepth = 32`，`JToken.Parse(..., new JsonLoadSettings { MaxDepth = MaxParamsDepth })` | `routes/mod.rs:30` `MAX_PARAMS_DEPTH = 32`，`handlers.rs:104,139` 两处校验 | 一致 |
| 空批量数组 | `RpcServer.ProcessAsync`：`if (json["params"] is JArray array && array.Count == 0) → InvalidRequest` 等路径，空数组视为 InvalidRequest | `handlers.rs` `process_array` 空 → `InvalidRequest` | 一致 |
| 无 `id`（notification） | `RpcServer.ProcessRequestAsync`：`if (!request.ContainsProperty("id")) return null;`（不返回响应） | `handlers.rs:216-220` `has_id == false → RequestOutcome::notification()` | 一致 |
| 缺 `method` / `params` 非数组 | → `InvalidRequest` | `handlers.rs` `process_object` 同 | 一致 |
| `FormatException` / `IndexOutOfRangeException` | → `InvalidParams.WithData(ex.Message)` | Rust 各处 `invalid_params(...)` | 一致（data 文本为 Rust 自撰，逐条不完全相同，属 LOW，未单列） |
| 其他异常 | → `RpcErrorFactory.NewCustomError(ex.HResult, ex.Message)` | Rust 多映射为 `-32603 internal_error` | C# 用 HResult（如 `InvalidOperationException` = -2146233079）；Rust `INVALID_OPERATION_CODE = -2146233079` 已在 stack-item 场景使用，其余场景建议逐步对齐 |
| `invokescript` script 编码 | `_params[0].AsString()` → `Convert.FromBase64String` | Base64 | 一致 |
| signers / witnesses 结构 | `SignersFromJson`：`account` 经 `AddressToScriptHash`；条目数 > `Transaction.MaxTransactionAttributes` → `InvalidParams.WithData("Max allowed witness exceeded.")`；`WitnessesFromJson` 过滤 invocation 与 verification 均为 null 的条目；`Signer`/`WitnessRule` 反序列化 `MaxSubitems = 16` | `parse_signers_and_witnesses` 走 `ParameterConverter` | **条目数上限校验与 `MaxSubitems = 16` 未确认在 Rust 侧实现** → 见"未能核实的项" |
| `getnep17balances` 地址参数 | `GetScriptHashFromParam` 按 `Length < 40` 分派 | 见 R2-13 | 偏差 |
| GET 请求 `params` | `RpcServer.ProcessAsync`：`params` 为 Base64 编码的 JSON，解码后用 `JToken.Parse` | `query_to_request_value`：先 Base64 解码，失败则回退按原始 JSON 解析，且要求结果为数组 | Rust 更宽松（允许明文 JSON）；建议保留，标注为扩展 |

---

## 五、JSON 输出格式

### 5.1 `getversion`

C# `RpcServer.Node.cs:113-145`（逐字段抓取）：

```
{ tcpport, nonce, useragent,
  rpc { maxiteratorresultitems, sessionenabled },
  protocol { addressversion, network, validatorscount, msperblock, maxtraceableblocks,
             maxvaliduntilblockincrement, maxtransactionsperblock, memorypoolmaxtransactions,
             initialgasdistribution,
             hardforks [ { name, blockheight } ] } }
```

- 分叉名：C# `StripPrefix(hf.Key.ToString(), "HF_")`；Rust `format!("{fork:?}").trim_start_matches("Hf")` → 二者结果一致（如 `Aspidochelone`）
- 分叉集合：C# 遍历 `system.Settings.Hardforks`（经 `EnsureOmmitedHardforks` 后包含未配置但被填 0 的项）；Rust 遍历 `Hardfork::all()` 并用 `protocol.hardforks.get(fork)` 过滤 —— **只要 `protocol.hardforks` 已经过 `ensure_omitted_hardforks`，二者一致**（配置文件加载路径 `protocol_settings.rs:123、353` 已调用）；但默认设置路径未调用 → R2-11
- Rust 额外输出 `protocol.standbycommittee`、`protocol.seedlist` → R2-07
- **C# 无 `magic`、无 `wsport`、无 `standbycommittee`、无 `seedlist`**（任务书中要求核对的 `magic`/`wsport` 在 C# v3.10.1 中确实不存在，Rust 未输出是正确的）

### 5.2 invoke 系列（`invokefunction` / `invokescript` / `calculatenetworkfee` 基础结构）

C# `RpcServer.SmartContract.cs:70-100` 输出顺序：`script`（Base64）、`state`、`gasconsumed`、`exception`、`notifications`、`[diagnostics]`、`stack`、`[session]`、`[tx|pendingsignature]`

- `state`：C# `json["state"] = session.Engine.State;` → `"HALT"` / `"FAULT"`；Rust `final_rpc_vm_state_string` → 同（NONE/BREAK 见 R2-17）
- `gasconsumed`：**C# `json["gasconsumed"] = engine.GasConsumed.ToString();` → 字符串**；Rust `system_fee.to_string()` → 字符串 ✅。数值来源：C# `ApplicationEngine.FeeConsumed => _feeConsumed.DivideCeiling(FeeFactor)`，`FeeFactor = 10000`；Rust `application_engine/state.rs:390-397` `fee_consumed() = (x + FEE_FACTOR - 1) / FEE_FACTOR`，`FEE_FACTOR = 10000` ✅
- `exception`：C# `GetExceptionMessage(ex) => exception?.GetBaseException().Message` → 无故障时 `null`；Rust `fault_exception().map_or(Value::Null, ...)` ✅
- `notifications`：`eventname` / `contract` / `state` ✅
- `diagnostics`：`invokedcontracts`（`hash`/`call` 递归）、`storagechanges[].{state,key,value}` ✅。**`state` 取值已核对一致**：C# `RpcServer.SmartContract.cs:152` `["state"] = entry.State.ToString()`，C# `TrackState` 枚举为 `None/Added/Changed/Deleted/NotFound`（`Neo/Persistence/TrackState.cs`）；Rust `helpers.rs:236` `format!("{:?}", trackable.state)`，Rust 枚举 `neo-storage/src/types/track.rs:8-21` 变体名与顺序完全一致 → 输出一致 ✅
- `stack[]`：InteropInterface 分支见 R2-04
- 钱包后处理：C# `ProcessInvokeWithWallet` 仅在 `session.Engine.State != VMState.FAULT` 时追加 `tx` / `pendingsignature`；Rust `process_invoke_with_wallet` 条件为 `vm_state != VMState::FAULT` ✅

### 5.3 `invokecontractverify`

C# `RpcServer.Wallet.cs:354-390`：

```
{ script (Base64, 无参数时为 Array.Empty<byte>() → ""), state, gasconsumed (string), exception, stack }
```

- `Signers = signers ?? new Signer[] { new() { Account = scriptHash } }`（`Scopes` 默认 0 = None）→ Rust `Signer::new(script_hash, WitnessScope::NONE)` ✅
- `Script = new[] { (byte)OpCode.RET }`、`engine.LoadContract(contract, md, CallFlags.ReadOnly)`、`engine.LoadScript(invocationScript, p => p.CallFlags = CallFlags.None)` → Rust 三项均一致 ✅
- 返回类型非 Boolean → `InvalidContractVerification("The verify method doesn't return boolean value.")` → Rust `contract_verify.rs:62-66` 文本逐字一致 ✅
- `verify` 方法查找 → R2-14

### 5.4 区块 / 交易 / 签名者 / 见证

- `Header.ToJson()`（`Neo/Network/P2P/Payloads/Header.cs`）→ `hash, size, version, previousblockhash, merkleroot, time, nonce(X16), index, primary, nextconsensus, witnesses`；Rust `header_fields_to_map`（`rpc_server_blockchain/mod.rs:621-662`）逐字段一致，`nonce` 用 `{:016X}` ✅
- `Utility.BlockToJson` = `block.ToJson()` + `tx[]`；`TransactionToJson` = `tx.ToJson()` + `sysfee`/`netfee`（字符串）→ Rust `transaction/json.rs` 一致 ✅
- `getblock` verbose 追加 `confirmations`，并在 `GetBlockHash(index+1)` 非 null 时追加 `nextblockhash` → Rust `block_to_json` 一致 ✅
- `Witness.ToJson()` → `{invocation, verification}`（Base64）→ Rust `neo-core/src/witness.rs:145-150` ✅
- `Signer.ToJson()` → `account, scopes[, allowedcontracts][, allowedgroups][, rules]` → Rust 一致，除 R2-05 / R2-06
- `ContractState.ToJson()` → `id, updatecounter, hash, nef, manifest`；`NefFile.ToJson()` → `magic, compiler, source, tokens, script, checksum` → Rust `rpc_contract_state.rs` / `rpc_nef_file.rs` ✅

### 5.5 `getapplicationlog`

C# `LogReader.cs`：block 日志优先（`blockhash` + `executions[]`），否则查 tx 日志（`txid` + `executions[]`）；无匹配 → `InvalidParams.WithData("Unknown transaction/blockhash")`；可选按 trigger 过滤（`Enum.TryParse(..., true, ...)`）。执行项字段 `trigger, vmstate, exception, gasconsumed, stack, notifications`（block 项的 `exception` 仅在 stack 序列化失败时才写入）。notification 的 `state` 为 `{type:"Array", value:[...]}`，递归引用时为字符串 `"error: recursive reference"`。

Rust `neo-rpc/src/server/rpc_server_application_logs.rs` + `neo-core/src/application_logs/service.rs`：**字段名、顺序、错误 data 文本、notification 包装形式、exception 的出现条件均一致** ✅

### 5.6 StateService

- 方法集合一致（6 个）✅
- `getstateheight` → `{localrootindex, validatedrootindex}` ✅
- `findstates` → `truncated` / `results` / `firstProof`（results > 0 时）/ `lastProof`（results > 1 时）✅（Rust 字段插入顺序不同，JSON 对象无序，无影响）
- `getstateroot` → `{version, index, roothash, witnesses[{invocation, verification}]}` ✅
- `verifyproof` 失败错误码 → R2-03

### 5.7 TokensTracker

- `getnep17balances` → `{address, balance[{assethash, name, symbol, decimals(string), amount, lastupdatedblock}]}` ✅
- `getnep11balances` → `{address, balance[{assethash, name, symbol, decimals(string), tokens[{tokenid, amount, lastupdatedblock}]}]}` ✅
- `getnep17transfers` / `getnep11transfers` → `{address, sent[], received[]}`，条目字段 `{timestamp, assethash, transferaddress(UInt160.Zero 时为 null), amount, blockindex, transfernotifyindex, txhash[, tokenid]}` ✅；时间窗默认值 → R2-12
- `getnep11properties`：白名单 `["name","description","image","tokenURI"]` 与 C# `_properties` 一致 ✅；白名单内取字符串、非白名单取 Base64 ✅；复合类型（`Array|Struct|Map`）跳过，与 C# `is CompoundType` 一致 ✅
- tracker 未启用时返回 `-32601 MethodNotFound`：C# `Nep17Tracker.cs:147` `_shouldTrackHistory.True_Or(RpcError.MethodNotFound);`，Rust `method_not_found()` ✅ 对齐

### 5.8 钱包方法

- `calculatenetworkfee` → `{networkfee: <string>}` ✅（C# `RpcServer.Wallet.cs`）
- `openwallet` → `File.Exists` 失败返回 `-303 wallet_not_found`，否则 `-304 wallet_not_supported`，成功返回 `true` ✅
- `getwalletunclaimedgas` → 遍历 `wallet.GetAccounts().Select(p => p.ScriptHash)`（**无 has_key 过滤**）累加 `UnclaimedGas(account, CurrentIndex+1)`，返回 `BigInteger.ToString()` → Rust 一致 ✅
- `getwalletbalance` → `{balance: <BigDecimal 字符串>}`；C# 走 `wallet.GetAvailable(snapshot, asset_id)`，其实现为 `GetAccounts().Where(p => !p.WatchOnly)`（`Neo/Wallets/Wallet.cs:331-335`）；Rust 过滤 `account.has_key()`。对 NEP-6 钱包二者等价（`WatchOnly ⇔ Key == null`）→ 可接受，未单列
- `getunclaimedgas` → `{unclaimed, address}`；Rust 用 `BigDecimal::new(unclaimed, NeoToken::decimals()=0)`，小数位为 0 时输出原始整数字符串，与 C# `BigInteger.ToString()` 一致 ✅

---

## 六、批量请求与 JSON-RPC 2.0 信封

| 检查项 | C# | Rust | 结论 |
|---|---|---|---|
| 响应含 `"jsonrpc": "2.0"` | `CreateResponse` 写入 | `success_response` / `error_response`（`routes/mod.rs`） | ✅ |
| id 回显（含字符串 id、null id） | `CreateResponse(id, ...)` 原样写入 | 原样写入 | ✅ |
| 错误响应结构 | `error = rpcError.ToJson()`（`code`/`message`/`data`） | `error_response` 同 | ✅ |
| 批量数组 | 逐元素处理，返回响应数组 | `process_array` | ✅（条数上限见 R2-16） |
| 无 id 的 notification | 返回 `null`，不产生响应 | `RequestOutcome::notification()` | ✅ |
| 方法名大小写 | `method.Name.ToLowerInvariant()` 注册，查找按原样 | `to_ascii_lowercase()` | ✅ |
| `DisabledMethods` | → `-600 AccessDenied` | `disabled.contains(&method_key) → access_denied()` | ✅ |
| 认证失败 | `CheckAuth` → `-600 AccessDenied`；HTTP 401 + `WWW-Authenticate: Basic realm="Restricted"` | `handlers.rs:258-262` + `build_http_response` | ✅ |

---

## 七、核对通过项（确认与 C# 一致）

1. **方法清单 100% 覆盖**：C# 全部 50 个方法在 Rust 中均已注册，命名与大小写规则一致；`getproofroot` / `getproofstate` / `getmempooldump` 在 C# v3 主线不存在，Rust 未实现是正确的。
2. **错误码表**：除 R2-01 / R2-02 / R2-10 外，全部 code + message 与 C# 一致；C# 的 -509 重复（InvalidScript / InvalidSize）为上游既定行为，Rust 已照抄。
3. **`RpcError.ToJson()` 序列化语义**：`message = "{Message} - {Data}"`、`data` 仅在非空时输出 —— 与 C# `ErrorMessage` 完全一致（C# 单测 `UT_RpcError.cs` 与 Rust 单测 `rpc_error.rs:242-251` 均断言 `-600 / "Access denied"`）。
4. **`RpcException` 桥接**：`neo-primitives/src/rpc_exception.rs` 的 `{code,message,data}` 与 `Display`（`"{message} - {data}"`）语义等价于 C# `RpcException`。
5. **JSON-RPC 2.0 信封与批量语义**：全部通过（见第六节）。
6. **`getversion` 分叉名生成**：`StripPrefix(hf.Key.ToString(), "HF_")` 与 `format!("{fork:?}").trim_start_matches("Hf")` 结果一致。
7. **`getversion` 不含 `magic` / `wsport`**：与 C# v3.10.1 一致（这两项本就不在 C# 输出中）。
8. **`ensure_omitted_hardforks` 填充逻辑**：与 C# `EnsureOmmitedHardforks`（遇到首个已配置项即 `break`）逐行等价。
9. **`gasconsumed` 为字符串且数值 = `DivideCeiling(feeConsumed, 10000)`**：与 C# `FeeConsumed` / `FeeFactor` 一致。
10. **`exception` 在 HALT 时为 `null`**：与 C# `GetExceptionMessage` 一致。
11. **`Header` / `Block` / `Transaction` / `Signer` / `Witness` / `ContractState` / `NefFile` 的 JSON 字段集合**：全部一致（`nonce` 的 `{:016X}`、`sysfee`/`netfee` 字符串、`allowedgroups` 用压缩公钥 hex 等细节均一致）。
12. **`getblock` verbose**：`confirmations` 必加、`nextblockhash` 按存在性添加 —— 一致。
13. **`getrawmempool`** → `{height, verified, unverified}`；**`getblocksysfee`**、**`gettransactionheight`**、**`getcommittee`**、**`getnextblockvalidators`**（`publickey` 用压缩公钥 hex、`votes` 为 `int`）均一致。
14. **`getnativecontracts` 顺序**：C# 按 `NativeContract.Contracts`（= 声明顺序，id 依次为 -1…−11）输出；Rust 注册顺序同为 ContractManagement→StdLib→CryptoLib→Ledger→NEO→GAS→Policy→RoleManagement→Oracle→Notary→Treasury（`neo-core/src/smart_contract/native/mod.rs:160-190`），且各 id 常量（`contract_management/mod.rs:64` = -1 … `treasury.rs:16` = -11）与 C# 相同，故 `sort_by_key(Reverse(id))` 的结果与 C# 声明顺序**完全相同** —— 此前怀疑的排序偏差不成立。
15. **`getapplicationlog`**：字段集合、`txid`/`blockhash` 结构、错误 data 文本 `"Unknown transaction/blockhash"`、notification `state` 的 `{type:"Array",value:[…]}` 包装与 `"error: recursive reference"` 回退 —— 全部一致。
16. **StateService 字段**：`getstateheight`、`findstates`（`truncated`/`results`/`firstProof`/`lastProof`）、`getstateroot` 结构一致。
17. **TokensTracker 字段**：balances / transfers / properties 的字段名、`decimals` 与 `amount` 为字符串、`transferaddress` 为 `UInt160.Zero` 时输出 `null`、NEP-11 `tokenid` 为 hex、tracker 未启用时返回 `-32601` —— 全部一致。
18. **`diagnostics.storagechanges[].state` 取值**：Rust `format!("{:?}", TrackState)` 与 C# `entry.State.ToString()` 一致（`None/Added/Changed/Deleted/NotFound` 变体名与顺序均已核对）。
19. **`validateaddress`** → `{address, isvalid}`，与 C# 一致（Rust 单测 `rpc_server_utilities.rs` 断言同一结构）。
20. **`traverseiterator` / `terminatesession`**：GUID session+iterator id、`MaxIteratorResultItems` 上限、错误文本 `"Invalid iterator items count {n}"`、返回值类型 —— 一致。
21. **`submitoracleresponse` 的错误映射**：OracleDisabled / OracleRequestFinished / OracleNotDesignatedNode / OracleRequestNotFound / InvalidSignature —— 一致。
22. **`openwallet` / `calculatenetworkfee` / `getwalletunclaimedgas` / `getunclaimedgas`** 的返回结构与错误码 —— 一致。

---

## 八、未能核实的项

1. **`SignersFromJson` 的条目数上限**：C# 在条目数 > `Transaction.MaxTransactionAttributes` 时抛 `InvalidParams.WithData("Max allowed witness exceeded.")`；Rust 走 `ParameterConverter` 走通用反序列化路径，未能静态确认是否实现了同一上限与同一 data 文本。
2. **`Signer` / `WitnessRule` 反序列化的 `MaxSubitems = 16` 限制**：C# 在 `Signer.FromJson` / `WitnessRule.FromJson` 上标注 `[MaxLength(16)]` 并由 `JsonSerializer` 强制；Rust 侧未找到对应校验代码，未能确认。
3. **`WitnessScope` 多标志组合的 C# 实际输出**：已确认 C# 输出为**字符串**且单标志时为枚举名（`docs.neo.org` 的 `getrawtransaction` 官方样例实测为 `"scopes": "CalledByEntry"` / `"None"` / `"FeeOnly"`）；但**未在 C# 源码或官方响应样例中找到多标志组合的实测值**。R2-05 中"分隔符为 `, `" 的依据是 .NET `[Flags]` 枚举 `ToString()` 的文档化行为 + JObject 直接赋枚举对象这一事实，**未取得 Neo 官方样例的实测确证**。建议用真实 C# 节点跑一次 `scopes = CalledByEntry|CustomContracts` 的交易确认。
4. **`getnep11properties` 在引擎非 HALT 时的返回**：C# `Nep11Tracker.GetNep11Properties` 的具体分支未能抓全（网络抓取时被截断），Rust 返回空对象 `{}` 是否与之等价未能确证。
5. **`final_rpc_vm_state_string` 的 NONE/BREAK 分支是否可达**：静态分析未找到能产出非 HALT/FAULT 终态的引擎路径，R2-17 的实际影响未知。
6. **`listplugins` 的 `interfaces` 字段值**：C# 各插件实际声明的 interface 名称集合未能逐一核对；Rust 用 `IPersistencePlugin` / `IStoragePlugin` 两个合成值。
7. **GET 请求路径的 `params` Base64 vs 明文**：Rust 允许两者，C# 仅 Base64；此差异是否有客户端依赖未能评估。
8. **`calculate_nep17_balance`（非 NEO/GAS 资产）**：C# 通过构造 `balanceOf` 批量脚本求和，Rust 同名函数的实现细节（gas 上限、失败回退）未能完整核对。
9. **`neo-modules` 基线只能取到 `master`**：RpcServer / StateService / TokensTracker / ApplicationLogs 的方法签名与错误码已用 `v3.7.5` 交叉校验一致，但 `v3.7.5` → 现在的 `master` 之间若有字段级改动（例如新增 protocol 字段）无法排除。若后续 `neo-modules` 补发 `v3.10.x` tag，需重跑本报告第二、五节。
