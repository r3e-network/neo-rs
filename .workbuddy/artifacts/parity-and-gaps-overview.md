# 剩余证明问题处理总结（2026-09-10）

## 结果：CI 14/14 全绿，并新增一项"正面回答完整性"的硬闸门

最终提交 `1c0707ff`：`14/14 success、0 failure、0 pending`。

提交链（本次增量）：`a858b8ec` → `da11f46f` → `1c0707ff`。

---

## 1. 新增：原生合约方法级对账（最核心的"完整性"证据）

**`scripts/verify-native-parity.py` —— 结果 118/118 完全对齐，已挂 CI 硬闸门并通过。**

为什么这个才是"完整性"的正解：测试数量、crate 数量都回答不了"是否完整实现"，因为测试只覆盖**有人想到要写测试的行为**。可辩护的做法是比较**表面**——C# 参考节点暴露的每个原生合约方法，Rust 是否同名暴露。

| 合约 | C# | Rust | 匹配 |
|---|---|---|---|
| ContractManagement | 10 | 10 | 10 |
| StdLib | 20 | 20 | 20 |
| CryptoLib | 13 | 13 | 13 |
| LedgerContract | 8 | 8 | 8 |
| NeoToken | 21 | 21 | 21 |
| GasToken | 5 | 5 | 5 |
| PolicyContract | 23 | 23 | 23 |
| RoleManagement | 2 | 2 | 2 |
| OracleContract | 5 | 5 | 5 |
| Notary | 8 | 8 | 8 |
| Treasury | 3 | 3 | 3 |
| **合计** | **118** | **118** | **118** |

### 过程中修掉的 4 个假差异（都是"凭直觉会错"的地方）

1. **C# 命名规则必须逐字复制**：`ContractMethodMetadata.cs:45-46` 是 `Name = attribute.Name ?? member.Name` 再首字母小写。猜就会被坑。
2. **声明匹配器要同时吃属性和元组返回**：`Symbol`/`Decimals` 是属性（`{ get; }`），只匹配 `Name(` 会跳过它们、错误吸附到后面无关的 `OnManifestCompose`；`GetCandidates` 返回元组 `(ECPoint PublicKey, BigInteger Votes)[]`，字符类不含括号会漏掉它。
3. **基准必须是 v3.10.1 而非 master-n3**：master-n3 是移动开发分支，已与被审计目标分叉。
4. **Rust 侧要跟随基类间接引用**：NEO/GAS 的 NEP-17 方法在 `FungibleToken::ft_nep17_methods()`，不在各自 `metadata.rs`——不跟随会误报 `balanceOf`/`totalSupply`/`transfer` 缺失。

### 反向验证（关键）

用临时副本把 StdLib 的 `atoi` 改名为 `atoiMUT`，脚本正确报出差异并 exit 1。**没有这一步，"PARITY" 可能只是脚本恒绿**，毫无意义。

### 这个检查的边界（不能夸大）

只比较方法的**存在性与命名**。不比较参数类型/元数、硬分叉激活窗口、语义、gas、call flags。绿 = **没有缺失或改名的方法**，≠ **行为等价**。

---

## 2. cargo-deny：拆掉软门控后暴露的真实状态

此前 `Dependency policy` 挂着 `continue-on-error`，**失败也会被吞成绿——不构成任何证明**。拆掉后得到两个确定结论：

**`licenses bans sources`：真通过**（还原为硬门控）。
此前 CI 失败的真因：`EmbarkStudios/cargo-deny-action` 把 `arguments` 排在 `command` **之前**（其 `action.yml` 的 `runs.args`），拼出 `... licenses bans sources check`，当前 cargo-deny 直接拒绝。**是工具问题伪装成策略违规**。改为下载固定 0.20.2 二进制直接调用后通过，与本地对 windows 及 linux 两个依赖闭包的验证一致。

**`advisories`：真失败，4 个真实 RustSec（非误报，本地复现 exit 1）**

| ID | 内容 |
|---|---|
| RUSTSEC-2025-0134 | rustls-pemfile 已归档无人维护（无安全升级） |
| RUSTSEC-2026-0098 | webpki：URI 名称约束被忽略 |
| RUSTSEC-2026-0099 | webpki：通配符证书名称约束被接受 |
| RUSTSEC-2026-0104 | webpki：CRL 解析可触发 panic |

**没有**加入 ignore 静默——屏蔽安全告警是维护者决策，真修是把 rustls 栈从 0.21 迁走（代码改动，非配置开关）。四项均限于 RPC/TLS 表面，**不在共识路径，不影响协议正确性**。已写入 workflow 注释并保留软门控，注明"不得视为已解决"。

---

## 3. NEO_EXECUTION_SPECS_REF：查清了，但仍需你配置

上游 `r3e-network/neo-execution-specs` **根本没有 v3.10.1 标签**（只有 v0.1.1 / v0.1.2），其最新提交对标 v3.10.0。所以该变量只能填 **commit SHA**，不能用标签名（`main`/`master` 被脚本刻意拒绝，因为是移动引用）。已在 workflow 中写明这一点与取值命令。**该闸门至今从未完整跑过，配好变量也只是必要条件，不算已验证。**

---

## 仍然不能宣称 100%，但缺口已大幅收窄

- ✅ **实现正确性**：强证据。无值错误级缺陷；38/38 预设对账 ALL MATCH；**原生合约方法表面 118/118 对齐**；CI 14/14 全绿；E1b/E2 解锁。
- ❌ **仍未证明**：
  1. 协议表面其余维度未对账——interop syscall、RPC 方法面、P2P 消息类型、VM 操作码（本次只做了原生合约）；
  2. **语义等价性**未证明（名字对齐 ≠ 行为一致）；
  3. `mainnet_block_*_repro` 18 例仍 `#[ignore]`（数据门控，需 C# 节点导入数小时）；
  4. `NEO_EXECUTION_SPECS_REF` 待配置且未验证；
  5. 覆盖引导 fuzzing 仅限 Linux CI；
  6. 4 项 RPC/TLS 依赖告警待你决策。

**可辩护的表述**："在审计界定范围内未发现正确性缺陷；原生合约方法表面与 C# v3.10.1 逐个对齐（118/118，已做变异测试验证检出能力）；所有可复现的验证通道已打通并全绿。语义等价性、其余协议表面与链上重放验证不在本次证明范围内。"
