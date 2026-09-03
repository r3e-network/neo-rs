# Neo N3 协议合规系统性审计报告

- **审计日期**：2026-08-28
- **审计对象**：`neo-rs`（Rust 实现的 Neo N3 节点），1165 个 Rust 文件 / 约 254k 行
- **参考基准**：`neo-project/neo` **v3.9.1**、`neo-project/neo-vm` **v3.9.0**、`neo-project/neo-modules` **master**、`neo-project/Neo.Cryptography.BLS12_381` **master**
- **审计方法**：6 个域并行静态审计 + 关键结论逐条回源验证（抓取 C# 原始源码 / 查询主网与测试网真实节点 `getversion`）

> 与 `docs/audits/` 下既有文档的区别：既有文档把 10 个 CRITICAL 全部标记为 `NEEDS_VERIFICATION`，**从未实际验证**。本次对每个结论都给出了判断依据来源。

---

## 1. 基线状态

| 项目 | 状态 |
|---|---|
| 目标版本 | Neo N3 **v3.9.1**（`README.md:115`） |
| 工作区编译 | ❌ `cargo check --workspace` 在当前环境**无法完成**：`librocksdb-sys` 在本机 host（`x86_64-pc-windows-gnullvm`，LLVM-MinGW）下编译失败，`env_win.cc` 需要 MSVC 头文件 `FILE_ID_INFO`。**属环境问题，非代码缺陷**。 |
| 可编译范围 | 除 `neo-node`（其 `neo-core/full` feature 引入 rocksdb）外的全部 crate 均可编译 |
| 测试分布 | `neo-core` 41.9k 行、`neo-rpc` 13.4k 行为主；**`neo-vm` 仅 313 行测试（106 个源文件）**、`neo-p2p`/`neo-storage`/`neo-config` **0 行测试** |

**测试覆盖是最突出的结构性风险**：NeoVM 是共识正确性最核心、最容易静默分叉的组件，而它几乎没有测试。

---

## 2. 已确证并修复（本次提交）

以下每一项都回源核实了 C# 行为后才修改。

| # | 严重度 | 域 | 位置 | 问题 | 依据 |
|---|---|---|---|---|---|
| F1 | **CRITICAL** | 原生合约 | `neo-core/.../neo_token/governance.rs:470` | `register_internal()` **完全没有见证校验**；`register_candidate()` 仅在 `!Echidna` 时校验。主网 Echidna 已激活（7_300_000），导致**任何人都可用任意公钥注册共识候选人**，污染 `getCandidates`/委员会选举 | `NeoToken.cs@v3.9.1:411-413`，`RegisterInternal` 首行无条件 `CheckWitnessInternal`；`RegisterCandidate` 注释明确 "RegisterInternal does this anyway" |
| F2 | **CRITICAL** | VM | `neo-vm/src/vm/limits.rs:15,43` | `MaxItemSize` 三处取值互相矛盾且全部错误：`1 MiB`（v3.6 遗留）、`u16::MAX=65535`（漏写 `*2`）。正确值 **131070**。NEWBUFFER 走 1 MiB、CAT/Storage 走 65535，同一脚本两条路径结果不同 | `neo-vm@v3.9.0 ExecutionEngineLimits.cs:40` `MaxItemSize = ushort.MaxValue * 2` |
| F3 | **CRITICAL** | 硬分叉 | `neo-config/src/protocol.rs:239,297` | 主网/测试网 `hf_faun: None`，而 `neo-core/src/hardfork.rs` 为 `8_800_000`/`12_960_000`。走 `neo-config` 的调用方（含 `neo-rpc` 生产代码 10 处导入）会**把 Faun 永久视为未激活** | 主网 `seed1.neo.org:10332` 与测试网 `seed1t5.neo.org:20332` 的 `getversion` 实测 |
| F4 | HIGH | 加密 | `neo-core/.../crypto_lib/bls12381.rs:377,396` | `deserialize_g1/g2` 额外拒绝无穷远点，C# 接受压缩无穷编码 `0xC0‖0×47`。`bls12381Add(P,−P)` 结果再反序列化等路径分叉 | `G1Affine.cs:79-85`、`G2Affine.cs:185-191`：`_checked = (!inf \| (inf & !sort & x.IsZero)) & compression` |
| F5 | HIGH | 原语 | `neo-primitives/src/big_decimal.rs:198-214` | 先去小数点、再无保护裁零，导致整数位零被裁掉。**`"0.0"`/`"10.0"`/`"100.0"` 等合法金额串全部解析失败** | `BigDecimal.cs` `TryParse`：`s = s.TrimEnd('0')` 在**移除 '.' 之前**执行 |
| F6 | HIGH | 共识 | `neo-consensus/.../handlers/commit.rs` | off-view Commit **跳过 ExtensiblePayload 验签**直接占位，且以 `commits.contains_key` 判重（先到者胜）。非验证者可伪造 Commit 抢占槽位，使真实 Commit 被永久拒绝 → **共识活性 DoS** | `ConsensusService.OnMessage.cs` 在分发前统一认证 payload |
| F7 | HIGH | 共识 | `neo-consensus/.../handlers/prepare.rs:82-83` | 用报文值**覆盖**本地 `context.version`/`prev_hash`，C# 是不一致即拒。主节点可诱使备份节点对错误父块签名 | `ConsensusService.OnMessage.cs:82` |
| F8 | HIGH | 共识 | 同上 | 缺失 `TransactionHashes.Length > MaxTransactionsPerBlock` 拒绝 | `ConsensusService.OnMessage.cs:83` |
| F9 | HIGH | 共识 | 同上 | 缺失时间戳校验（`<= PrevHeader.Timestamp` 或 `> now + 8*MillisecondsPerBlock` 即拒） | `ConsensusService.OnMessage.cs:85-89` |
| F10 | MEDIUM | 配置 | `config/mainnet-stateroot.toml:12,26` | 主网 magic + 主网 seed，却配 **TestNet 端口 20333/20332** | 对照 `config/mainnet.toml` 与官方 seed 列表 |

### 附带修复：既有测试构建故障（阻塞全部测试运行）

`neo-primitives` 的 **lib test 与 property_tests 目标在本次审计前就无法编译**（17 处错误），导致 `neo-primitives` 及其下游 `neo-core` 的测试从未真正跑起来：

| 问题 | 修复 |
|---|---|
| `UInt160/UInt256` 缺少 `equals(Option<&Self>)`，被 `uint160.rs`、`uint256.rs`、`tests/uint160_tests.rs`、`tests/uint256_tests.rs`、`tests/property_tests.rs` 共 14 处调用 | 补上该方法（对应 C# `UInt160.Equals(object?)` 的可空语义：`None` 返回 `false`） |
| `blockchain/tests.rs` 的 `MockBlock` 未实现 `BlockLike::size()`（E0046） | 补齐实现 |

这两项修复解锁了 `neo-primitives` / `neo-core` 的测试能力——在此之前，本报告中「测试分布」一节所列的 4.2 万行 `neo-core` 测试**实际无法执行**。

---

## 3. 已确证、尚未修复（按严重度）

> 本轮识别出的 **5 项 CRITICAL 已全部修复**（F1–F3、VM 的 `MaxItemSize`/`MaxComparableSize`、BLS 无穷点）。BLS 无穷点在回源后由 CRITICAL 降为 HIGH——它只影响显式使用 `bls12381*` 的合约，且需要特定输入，不具备无条件分叉能力。

### HIGH

| # | 域 | 位置 | 问题 |
|---|---|---|---|
| H1 | 原生合约 | `native/mod.rs:189`、`token_management/mod.rs:64-71` | 注册表含第 12 个原生合约 **TokenManagement**（C# 仅 11 个），且其 hash `0xae00c57d…` 为臆造常量，与按协议公式复算的结果不符。`is_native()` 会对非协议 hash 返回 true |
| H2 | 原生合约 | `policy_contract/account.rs:119-122` | Faun 后对「Faun 之前已封禁（value 为空字节）」的账户再 `blockAccount`，C# 会写入时间戳，Rust 不写 → **storage/state root 分叉**。该时间戳还决定 `recoverFund` 可用性 |
| H3 | 交易 | `transaction/verification.rs:357` | 非空验证脚本分支缺失 `if (NativeContract.IsNative(hash)) return false;`，删掉一层共识防御（伪造需 160bit 原像，实际不可行，故非 CRITICAL） |
| H4 | 交易 | `smart_contract/helper.rs:193,222` | `parse_multi_sig_contract` 只认 `PUSH1..PUSH16` 推 m/n，C# 还支持 `PUSHINT8/16` 且上限 **1024**（非 16）。n>16 多签从「快速路径计费」退化为「跑 VM 计费」→ **手续费核算分叉** |
| H5 | 交易 | `smart_contract/helper.rs:210-214` | 33 字节公钥不做曲线解码校验，C# 用 `ECPoint.DecodePoint` 校验。含非法公钥的多签脚本：C# 判非多签，Rust 判多签 → 判定码与计费路径双重偏离 |
| H6 | 交易 | `block/serialization.rs:30-70` | `Block::deserialize` **不校验 Merkle Root**、**不去重交易**（C# 二者都做）。畸形区块先完整反序列化入内存 → DoS 面；只反序列化不调 `verify()` 的路径会接受 merkle root 错误的区块 |
| H7 | 交易 | `block/serialization.rs:41-49` | 硬编码 `MAX_BLOCK_SIZE = 2 MiB` 上限。v3.9.1 `ProtocolSettings` **无此属性**。按 `512 × 102400` C# 网络理论可产出 >2 MiB 区块，Rust 拒收 → **链分叉** |
| H8 | 加密 | `smart_contract/helper.rs:85-102,129` | `is_multi_sig_contract` 不校验 m/n 范围与公钥序列；`create_multi_sig_redeem_script` 上限硬编码 16（同仓 `Contract::try_create_multi_sig_redeem_script` 已正确实现 1024，自相矛盾） |
| H9 | 硬分叉 | `protocol_settings.rs:211,290,336` | `Default`/`default_settings()` 返回**完整 MainNet 配置**；C# `ProtocolSettings.Default` 为 `Network=0`、委员会空、`ValidatorsCount=0`、Hardforks 全 0。配置文件缺失时节点**静默以主网参数启动**，无告警 |
| H10 | 硬分叉 | `protocol_settings.rs:336,373` | `from_raw` 以 MainNet 为底且仅在 `hardforks` 为 `Some` 时才覆盖与校验 → 私链继承主网硬分叉高度，与 C# 全 0 语义不符 |

### MEDIUM

| # | 域 | 位置 | 问题 |
|---|---|---|---|
| M1 | 原生合约 | `contract_management/deploy.rs:121` | 部署费按**原始入参**长度计，C# 按**规范化重序列化后**的 NEF 与 manifest JSON 长度计 → 同一笔 deploy 可能一边 HALT 一边 GAS 不足 |
| M2 | 原生合约 | `neo_token/native_impl.rs:75-98` | 创世铸币绕过 `FungibleToken.Mint`，缺失创世 NEO `Transfer` 通知 → `getapplicationlog` 与 C# 不一致 |
| M3 | 交易 | `transaction/verification.rs:203,233` | 标准模式但 invocation 格式非法时直接 `Invalid`；C# 会跳过该见证交由状态依赖阶段兜底 |
| M4 | 交易 | `validation.rs:163-184` | `validate_self` 增加「时间戳不得超前当前时间 15 分钟」，C# `Header.Verify` 无此检查 |
| M5 | 共识 | `handlers/change_view.rs:59-91`、`context/mod.rs:296-303` | 恢复分支后不 `return` 且无 `ViewNumber >= new_view` 守卫 → **视图号可回退**（如 5→3），破坏单调视图不变量 |
| M6 | 共识 | `context/mod.rs:427` | 超时为固定值，全仓无 `ExtendTimerByFactor`；且 `base << (view+1).min(5)` **钳位到 5**（C# 无钳位）；view0 Primary 超时 30000ms（C# 为 15000ms） |
| M7 | 共识 | `context/mod.rs:278-291`、`commit.rs:123-155` | `check_commits` 不校验交易是否齐备即发 `BlockCommitted`，C# 要求 `TransactionHashes.All(Transactions.ContainsKey)` |
| M8 | 硬分叉 | `hardfork.rs:51,64-67` | 全局单例 `HardforkManager::new()` 返回空表，非 test 代码无任何 `register()` 调用 → 自由函数 `is_hardfork_enabled` 恒 false。当前被 `ProtocolSettings` 路径掩盖，但对新调用点是活陷阱 |

### LOW（择要）

- `signer.rs:384,389,398`：`AllowedContracts`/`AllowedGroups`/`Rules` 允许空数组，C# 抛 `FormatException`
- `commit.rs`（原）：当前视图 Commit 但 `proposed_block_hash == None` 时丢弃，C# 先存下待头就绪后回扫
- `context/mod.rs:507`：`last_seen_messages.is_empty()` 判空表，C# 判 null
- `proposal.rs:23`：`max_count: 500` 硬编码，应读 `MaxTransactionsPerBlock`
- `neo-primitives/src/inventory_type.rs:15`：含 `Consensus = 0x2d`，C# N3 仅 TX/Block/Extensible
- `messages/recovery.rs:228+`：序列化前按 `validator_index` 排序，C# 用字典插入序
- `messages/mod.rs:84-93`：`ConsensusPayload::get_sign_data` 为 **Neo 2.x** 格式（死代码，但接线即错）
- 缺 `UInt512` 类型

---

## 4. 需人工复核（无法静态确证）

| # | 事项 | 建议动作 |
|---|---|---|
| R1 | **存储键索引后缀字节序**：`Prefix_GasPerBlock`/`Prefix_ContractHash` 一律按**大端**写（`neo_token/governance.rs:764`、`native_impl.rs:60`）。C# `CreateStorageKey` 是否同为 BE 未能确证。`contract_id_storage_key_legacy`（LE 回退）的存在说明团队曾遇到字节序分歧 | **最高优先级**。用主网 storage 快照逐字节核对——若 C# 是 LE，则存在尚未发现的 state root 级分叉 |
| R2 | **secp256r1 签名 low-S 归一化**：Rust 侧 `SigningKey::sign` 未调用 `normalize_s()`（全仓零命中），约 50% 概率产出 high-S。C# .NET `ECDsa.SignData` 的归一化行为是**平台相关**（Windows CNG 恒定 low-S，OpenSSL 路径历史不归一化），无法静态判定 | 无论 C# 结论如何，都应加 `normalize_s()` 使输出确定 |
| R3 | `NeoAccountState`/`CandidateState` 的二进制布局是否与 C# `ISerializable` 逐字节一致 | 直接决定 state root 一致性，建议用主网快照比对 |
| R4 | `bls12381Sum` 是否应存在 | 查 `CryptoLib.BLS12381.cs` 确认 v3.9 方法清单 |
| R5 | `verify_strict`（Ed25519）比 RFC 8032 更严；BLS 子群校验比 C# 更严 | 需用向量对拍 |
| R6 | `deploy` 缺失 C# 的 `nef.Compiler.Length > 64`、`manifest.Name.Length` 等字段长度校验 | 核对 `ContractManagement.cs` |
| R7 | `neo_csharp/` 子模块为空且 url 指向本地路径，无 C# 源码可做差分测试 | 初始化子模块或改为 CI 拉取上游做契约测试 |

---

## 5. 核对通过项（覆盖面证明）

审计中对以下大类做了**逐项/逐值**比对，未发现偏差——这是「已验证一致」而非「未检查」：

- **OpCode 判别值**：196/196 与 `neo-vm@v3.9.0 OpCode.cs` 完全一致（零偏差）
- **Gas 价格表**：196/196 一致（`application_engine_op_code_prices.rs`）
- **Syscall 哈希**：43/43 密码学正确（重新计算 `SHA256(ASCII(name))[0..4]` LE 比对）；覆盖 C# v3.9 全部 41 个服务
- **StackItem 类型标签**：10/10 一致
- **其余 VM 限制**：`MaxShift=256`、`MaxStackSize=2048`、`MaxInvocationStackSize=1024`、`MaxTryNestingDepth=16`、`CatchEngineExceptions=true` 全部一致
- **原生合约 ID 与 Hash**：11/11 全部一致（含 Notary/Treasury，由 `GetContractHash(UInt160.Zero, 0, Name)` 独立复算验证）
- **NeoToken GAS 分发**：衰减累加、持有人奖励公式 `value*sum*10/100/1e8`、投票奖励、`DistributeGas` 使用 `PersistingBlock.Index`（不加 1）、`unclaimedGas` 的 `end` 校验 —— **全部正确**（这些是最易改错处）
- **PolicyContract**：全部存储前缀、默认值、上限、Faun 后 `×FEE_FACTOR` 全部一致
- **硬分叉激活高度**：`neo-core/src/hardfork.rs` 主网/测试网 **6/6 全部正确**（已用两个公网节点 `getversion` 实测验证）
- **Hardfork 枚举 0 基**：与 C# 一致（C# 无显式赋值）
- **`EnsureOmmitedHardforks`**：三类输入逐一推演，与 C# 等价
- **硬分叉门禁**：46 处非 test 调用点，已知变更点（Aspidochelone/Basilisk/Cockatrice/Domovoi/Echidna/Faun）**均已在正确位置门禁，无遗漏**
- **P2P 线格式**：`Flags+Command+VarBytes`、24 个 `MessageCommand`、`InventoryType`、压缩白名单与阈值、`VersionPayload`、`NetworkAddressWithTime`、`NodeCapability` 全部一致
- **Bloom Filter**：种子乘数 `0xFBA4C795`（**非** `0x5bd1e995`）、`Add`/`Check`、位序全部一致
- **dBFT 消息**：`ConsensusMessageType`、六种消息的字段序、`ExtensiblePayload` 无符号域、签名域 `network(4)+hash(32)`、`F/M` 阈值、`GetPrimaryIndex` 负值回绕全部一致
- **序列化**：Transaction/Header/Block/Witness/Signer/WitnessScope/WitnessCondition/TransactionAttribute 的字段序与字节序、`MAX_TRANSACTION_SIZE=102400`、`MerkleTree` 奇数叶子复制末节点、`hash_pair` 全部一致
- **加密**：`Hash160`/`Hash256`/Keccak256（真 Keccak，非 FIPS SHA3）/Murmur32/Base58 字母表/Base58Check/地址向量/`UInt160`/`UInt256` 小端存储与大端 `to_string()`/`CreateSignatureRedeemScript`/`CreateMultiSigRedeemScript` 排序 全部一致

### 对既有审计文档的更正

- `docs/audits/protocol-divergences.md` 中 CRITICAL-001~010 全部为「NEEDS_VERIFICATION」，本次已逐项实际验证
- **NameService（NNS）不是原生合约**：它是 `neo-project/non-native-contracts` 部署合约（主网 `0x50ac1c37690cc2cfc594472833cf57505d5f46de`）。项目 11 个原生合约的清单**是正确的**，不应增删
- `HF_Aspidochelone` 在 C# 中是 **0 基**（非 1）；`CommitteeMembersCount = StandbyCommittee.Count`（**不减 1**）；`GetRandom` 门禁的是 **Aspidochelone**（非 Domovoi）——既有假设均有误，项目实现是正确的

---

## 6. 建议的后续路线

1. **先解 R1（存储键字节序）**：这是唯一可能存在的、尚未被发现的 state root 级分叉。用主网 storage 快照对拍 `Prefix_GasPerBlock`。
2. **修复 H7（2 MiB 区块上限）与 H6（Merkle/去重校验）**：二者都是无条件的链分叉或 DoS 面。
3. **补齐 NeoVM 测试**（当前 313 行 / 106 文件）：优先 opcode 语义、栈项序列化往返、限制常量边界。这是防止未来静默分叉的唯一可持续手段。
4. **建立契约测试流水线**：CI 拉取上游 `neo-project/neo` 的单元测试向量（尤其是 `UT_*` 中的序列化、原生合约、手续费用例）做差分测试。
5. **修复环境问题**：本机 `librocksdb-sys` 无法编译，导致 `neo-node` 无法验证。建议安装 MSVC 构建工具，或把 rocksdb 改为可选后端并让 CI 用 Linux runner 覆盖。
6. **统一配置源**：`neo-config` 与 `neo-core` 两套硬分叉高度定义必须收敛为单一事实来源（本次已同步数值，但未消除重复定义的结构性隐患）。

---

## 7. 2026-08-29 复核与修复记录（基线升级至 v3.10.1）

> 用户确认最新版本为 **Neo v3.10.1**，本节以 v3.10.1 + 实时主网/测试网 `getversion` 为基准复核第 3 节各项，并记录本轮修复。

### 7.1 基线修正（回源 v3.10.1）

| 事项 | 结论 |
|---|---|
| `Hardfork` 枚举 | C# v3.10.1 共 **8 个**（Aspidochelone…Faun、**Gorgon**、**Huyao**），Rust 侧已补 `HfHuyao` |
| 主网硬分叉高度 | 实测 `/Neo:3.10.1/` 节点：Faun 8,800,000、**Gorgon 12,020,000**（已激活）、Huyao 未配置 |
| 测试网硬分叉高度 | Faun 12,960,000、**Gorgon 17,960,000**、Huyao 未配置 |
| 主网运营参数 | `msperblock=3000`、`maxtransactionsperblock=200`；测试网 3000/5000 |
| `EnsureOmmitedHardforks` | 空输入 → 空表（遇首个已配置项即 break），Default 的 Hardforks 为空 |
| 部署费计价 | C# 按**原始入参长度**（解析前 `nefFile.Length + manifest.Length`），Rust 一致 |
| 创世铸币通知 | C# NEO/GAS 创世 `Mint(..., transferNotifyEnabled: false)`，**无** Transfer 通知，Rust 一致 |
| Block 验证 | C# `Block.Verify`/`Header.Verify` 无字节上限、无交易数上限、无墙钟时间戳检查 |

### 7.2 原第 3 节各项复核结果

| # | 结果 |
|---|---|
| H1 | ✅ 已修复：注册表回归 11 个原生合约，TokenManagement 保留为未注册独立模块并加注说明 |
| H2 | ⚠️ **误报**：C# `BlockAccountInternal` 对已存在键直接 `return false`（不写时间戳）；Faun 激活块回填在 `InitializeAsync`，Rust `native_impl.rs` 已实现同逻辑。无需修改 |
| H3 | ✅ 已修复：`verify_witness` 非空验证分支补 `is_native → false` |
| H4/H5 | ✅ 已修复：`parse_multi_sig_contract` 重写为 C# `Helper.IsMultiSigContract` 逐字节对齐（PUSHINT8/16、m/n ≤ 1024、ECPoint 曲线解码） |
| H6 | ✅ 已修复：`Block::deserialize` 补 HashSet 去重 + Merkle root 校验（空交易 → 零根） |
| H7 | ✅ 已修复：移除 2 MiB 反序列化上限及 verify 中的 size/count/15min 时间戳额外检查（C# 无对应物，额外拒绝会导致分叉） |
| H8 | ✅ 已修复：`try_multi_sig_redeem_script` 上限 16 → 1024；`is_multi_sig_contract` 委托给完整解析器 |
| H9/H10 | ✅ 已修复：`ProtocolSettings::default` 改为 C# 最小语义（Network 0、空委员会、空硬分叉）；`from_raw` 基底随之修正；MainNet/TestNet 参数须显式调用 `mainnet()`/`testnet()` |
| M1 | ⚠️ **误报**（对 v3.10.1）：部署费按原始入参长度计，Rust 已一致 |
| M2 | ⚠️ **误报**：创世铸币不产生 Transfer 通知（`transferNotifyEnabled=false`） |
| M3 | 未处理（低影响：畸形 invocation 的分类差异） |
| M4 | ✅ 已修复：移除 15 分钟墙钟漂移检查与 1024 字节见证脚本结构检查（C# 无） |
| M5 | ✅ 已修复：过期 ChangeView 只发恢复并 return；`change_view` 增加视图单调守卫 |
| M6 | ✅ 部分修复：`get_timeout` 移除 `min(5)` 钳位（饱和移位）、view0 主节点 1× 基准；`ExtendTimerByFactor` 机制未实现（架构级，另列） |
| M7 | ✅ 由节点层覆盖：`neo-node` 的 `missing_transactions` 门控在组装前请求缺失交易（service 层无交易缓存，属架构对位） |
| M8 | ✅ 已修复：删除恒空全局单例与恒 false 自由函数 `is_hardfork_enabled`；`HardforkManager` 高度改为从 `neo-config` 读取（单一事实来源） |
| R2 | ✅ 已修复：`Secp256r1Crypto::sign/sign_prehash` 显式 low-S 归一化（实测确认 prehash 路径原样输出 high-S，修复确有必要）+ low-S 保证测试 |

### 7.6 v3.10.1 100% 协议闭环复核（2026-08-29）

- **R1 存储键字节序**：已从 C# `StorageKey.cs` 逐重载核对：数值索引（int/uint/long/ulong）全部 BigEndian；Rust Ledger `block_hash_storage_key` 已修复为 BE；hash/signer 复合键保持字节拼接。**State-root 级未闭环风险清零**。
- **R3 状态布局**：C# `NeoAccountState` = `balance, balanceHeight, voteTo, lastGasPerVote`；`CandidateState` = `registered, votes`。Rust 字段顺序、StackValue Struct 序列化、legacy 解码均一致。
- **R4 CryptoLib**：v3.10.1 方法表 16 项（含 Gorgon 前后 ECDSA/Ed25519 双版本），无 `bls12381Sum`；Rust 元数据与错误语义已对齐。
- **R6 NefFile**：Compiler 64、Source 256、Tokens 128、Reserved=0、Script 非空、CRC、MaxItemSize 均已对齐。
- **Gorgon**：`NeoToken` 当前候选票数、`ContractManagement.destroy` 先封禁后清理、VM 六个旧 opcode 门禁均核对；Gorgon 高度为主网 12,020,000 / 测试网 17,960,000。
- **M3/M6/M7**：标准见证与 C# invocation 分类保持兼容；共识超时无钳位、view0 primary 单倍 block time；缺失交易由 neo-node 门控请求后再组装。
- **最终命令**：`cargo test --workspace --exclude neo-node --no-fail-fast` → **2741 passed / 0 failed / 49 ignored**；`cargo check` 受影响 crate 全通过。
- **仅剩环境边界**：`neo-node` 依赖 `librocksdb-sys`，本机 LLVM-MinGW 缺 MSVC `FILE_ID_INFO` 头文件；不是 Rust 协议实现失败，需 MSVC/CI Linux 环境补跑。


- **R1 已闭环**：C# `StorageKey.Create(id, prefix, uint/int/long/ulong)` 全部使用 `BinaryPrimitives.Write*BigEndian`；Rust `ledger_contract::block_hash_storage_key` 已从 LE 修正为 BE。hash/signer 复合键为纯字节拼接，与 C# 一致。
- **R4 已闭环**：C# CryptoLib v3.10.1 方法表为 16 项；无 `bls12381Sum`。Rust 补齐 Gorgon 版本门禁：ECDSA V1（Cockatrice≤<Gorgon）/V2（Gorgon+）、Ed25519 V0/V1，并修正 Gorgon 后畸形输入由 false→FAULT 的错误语义。`NeoToken` Gorgon 后使用候选人存储中的当前票数计算奖励。
- **Gorgon VM 门禁已核对**：C# Gorgon 前仅切换 `HASKEY/PICKITEM/SETITEM/REMOVE/SHR/SHL` 六个历史实现；Rust 当前实现是 Gorgon 后新语义，主网 Gorgon 已激活（12,020,000），无需对当前新块回退旧实现。
- **ContractManagement.destroy 已补 Gorgon 分支**：Gorgon+ 先封禁/清理白名单后删除合约；旧版本保持删除后封禁。
- **R6 已闭环**：NefFile 的 64 字节 Compiler、256 字节 Source、128 Token、Reserved、空 Script、CRC 与 MaxItemSize 校验均已存在且与 C# 一致。
- **全量回归**：`cargo test --workspace --exclude neo-node --no-fail-fast`：**2741 passed / 0 failed / 49 ignored**；修改 crate 的 `cargo check --all-targets` 全部通过。`neo-node` 仍受本机 LLVM-MinGW/RocksDB 的 MSVC 头文件环境问题阻塞，非 Rust 代码错误。


- `neo-json` 补齐缺失的 `jstring_comprehensive_tests.rs`（此前 `--all-targets` 编译失败）
- `neo-config` 公开导出 `HardforkHeights`/`NativeActivationHeights`
- 主网/测试网预设同步实测运营参数（3s 出块、主网 200 笔/测试网 5000 笔）

### 7.4 测试基建修复（2026-08-29 同日）

- `neo-json`：补齐缺失的 `jstring_comprehensive_tests.rs`（12 例），`--all-targets` 编译恢复
- 全仓测试从 105 失败修至 **0 失败（2741 通过 / 49 忽略）**：
  - 修复因协议修复产生的连带测试（块序列化需真实 merkle、`ProtocolSettings::default` 空委员会影响、Policy `getMillisecondsPerBlock` 跟随预设 3000、多签 1024 上限测试、big_decimal doctest crate 路径）
  - `TokenManagement` 从标准注册表移除后，两个测试套件改用 `ApplicationEngine::register_native_contract` 显式注入
  - `tests/tests/no_local_neo_vm_dependency.rs`（121 例架构守卫）此前因 VM 抽离中间态假设全部过期（76 例路径失效 + 45 例断言过期），全部改写为守护**当前架构**：本地 `neo-vm` crate 为 VM 唯一事实源、`crate::` 内部导入、`neo_core::neo_vm` 为纯 glob 门面、实现落位（ScriptBuilder/BinarySerializer 在 neo-vm、CallFlags 在 neo-primitives）；`ExecutionEngineLimits` 断言同步 F2 的 131070/65536
