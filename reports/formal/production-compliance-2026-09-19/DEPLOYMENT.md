# Neo-rs 生产部署清单（以形式验证证据为纲）

> **日期**：2026-09-19
> **协议基线**：Neo N3 v3.10.1
> **文档定位**：本文件以 `reports/formal/` 下的**形式验证/差异验证证据**为纲，列出生产部署前必须满足的验证门槛。
> **诚实声明**：本文件**不宣称"已生产就绪"**。已由机器验证的证据与尚未覆盖的盲区并列呈现，作为部署决策的准入依据。
> **与既有文档的关系**：`docs/DEPLOYMENT.md`、`docs/DEPLOYMENT-READINESS-REPORT.md`、`docs/DEPLOYMENT-VALIDATION-CHECKLIST.md` 侧重部署操作/性能优化，本文件侧重"**证据驱动的验证门槛**"，不重复其操作步骤。

---

## 0. 判定原则

- **准入 = 已证门槛（必过）+ 盲区声明（必须书面接受）**，二者缺一不可。
- 任何"已验证"表述必须可追溯到一份机器验证产物（diff runner exit 0、coqchk exit 0、Mimosa scan 封印）。
- 不可用"单元测试通过 / 优化完成 / 性能达标"替代**协议语义一致性**证据——两者回答不同问题。
- 参考实现基线**锁定 v3.10.1**，C# runner csproj 已固定并注释 "Do NOT float"。

---

## 1. 证据清单（已通过的验证门槛）

以下门槛**已有当前代码生成的机器证据**，部署前必须逐项核验为 PASS。证据来源及复现命令详见 `reports/formal/`。

| # | 门槛 | 证据 | 结果 | 覆盖范围 | 边界（必须同时声明） |
|---|------|------|------|----------|----------------------|
| G1 | **VM 指令执行语义一致**（合成） | `vm-diff-current-vs-3.10.1-2026-09-18.md` §2 | 99/99 PASS, 0 different | 算术/比较/位/栈/流/类型/字节/转换/元数据，含故障分支 | runner 为 **bare engine**：无宿主、无 syscall、无存储 |
| G2 | **VM 指令执行语义一致**（链上） | 同上 | 708/708 PASS, 0 different | 27 个 MainNet 真实区块的 invocation/verification 脚本，跨 hardfork 高度 | 同上；syscall 脚本在两侧均预期 FAULT，只验证"未注册 syscall 报错一致" |
| G3 | **Wire 序列化字节级一致** | `serialization-wire-diff-vs-3.10.1-2026-09-18.md` | 42/42 PASS（Rust 内部 roundtrip）+ 42/42 字节一致（C# 重编码） | Block/Transaction/Header 及其 varint/varbytes/签名者/属性/witness 子结构；varint 全宽度边界 | 合成样本；未覆盖超长 OracleResponse 载荷、WitnessCondition 复合嵌套（仅 Boolean）、超限负向拒绝路径 |
| G4 | **带宿主 syscall/native 差异**（对齐子集） | `syscall-diff-expanded-2026-09-18.md` | 27/27 PASS | 含 GasLeft 精确 gas 记账、CheckWitness 正路径、CurrentSigners、GetScriptContainer、Log | 5 个向量未纳入对齐（见 §3 盲区） |
| G5 | **Coq 门禁（机器可验证证明）** | `coverage-status-2026-09-18.md` | 25/34 通过（coqc 0 + coqchk 0，无 assumed axioms） | varint/编码、opcode price 表 256 值、区块验证、tx attribute、Merkle、共识消息 roundtrip 等 | 9 个模型未通过；Coq 是**抽象模型/契约，非 Rust 可执行文件的精化证明**；哈希为显式参数、不主张抗碰撞 |

> 权威判定标准：`formal/ci/check_coq.py --all`（隔离目录独立 `coqc` + `coqchk`，不复用旧 `.vo`）。
> 工具链：Coq 8.18.0 / OCaml 4.14.1；源码 SHA-256 见 `reports/formal/coq-baseline-now/summary.json`。

---

## 2. 生产部署前必须满足的验证门槛（准入 Checklist）

以下为**上线准入**，全部为必选项；未满足即**阻断**。

### 2.1 共识/状态一致性门槛（G1-G4 关联）

- [ ] **G1+G2 PASS**：VM 纯指令语义与 v3.10.1 一致（当前通过）。
- [ ] **G3 PASS**：wire 序列化字节级一致（当前通过）。
- [ ] **G4 PASS（27 对齐子集）**：带宿主 syscall 语义一致（当前通过）。
- [ ] **【阻断项】存储/原生合约价值转移正路径尚未验证** —— 见 §3 S1/S2。在补齐 S1/S2 之前，**不得将共识节点作为独立验证者参与主网**（即不得单方面以其状态为权威）；可作为观测/跟随节点运行，但需明确其状态不得作为分叉裁决依据。
- [ ] **【阻断项】G5 中 9 个未通过 Coq 模型** —— 见 §3 S5。任何涉及 witness/多签/状态根/ABI/mempool 的变更上线前，必须关联对应模型的验证状态。

### 2.2 部署完整性门槛（承接 docs/DEPLOYMENT.md，不重复操作细节）

- [ ] 使用 **production profile** 构建，产物为 release/stripped/LTO 二进制。
- [ ] `make preflight` + `--check-all` 通过。
- [ ] 配置**不包含硬编码可用密钥**；RPC 认证凭据、共识私钥、TLS 密码**一律来自环境变量或密钥服务**（详见 `COMPLIANCE.md`）。
- [ ] 主网/测试网 magic、端口（10333/10332 vs 20333/20332）核对无误，防跨网污染。
- [ ] 日志开启 JSON 结构化 + 轮转；审计日志落盘（详见 `COMPLIANCE.md` §审计）。
- [ ] 监控/告警已接（`/healthz`、`/readyz`、`/metrics`）；链头延迟、出块参与度、内存/磁盘有阈值。
- [ ] 备份与回滚预案已演练（详见 `INCIDENT-RESPONSE.md`）。

### 2.3 上线的阶段性（诚实建议）

1. **阶段 A（当前可达）**：作为**跟随/观测节点**运行，G1-G4 已证，G5 部分已证。
2. **阶段 B（补齐存储/原生正路径验证后）**：作为**独立验证者**参与，但其状态仍需与参考网络交叉校验。
3. **阶段 C（全量状态一致验证完成）**：才可声明"状态一致、可作分叉裁决/权威"。

> 阶段 C 依赖的"区块状态（MPT/state root/原生合约余额/存储键值）一致"目前**尚无机器证据**。

---

## 3. 部署风险 / 待验证盲区（如实列出，不许假装全绿）

### S1. 存储 Get/Put/Delete/Find 正路径 —— **未覆盖**
- **现状**：`syscall-diff-expanded` 仅覆盖了 `Storage.GetContext` 的负路径（两侧一致 FAULT）；正路径需先部署一个带存储的合约并在两侧种入相同状态，**未做**。
- **风险**：存储读写是原生合约价值转移（NEO/GAS 余额、`totalSupply`/`balanceOf`）的底层语义。若两侧存储语义分歧，会在状态根/余额上产生**静默分叉**，且不在 G1-G4 覆盖内。
- **待办**：构造"合约部署 + 存储种入 + Get/Put/Delete/Find"的对齐向量，纳入 syscall 差异验证。

### S2. 原生合约（NEO/GAS/Crypto/Contract）读侧值转移 —— **部分覆盖/有真实解析差异**
- **现状**：`System.Contract.Call` 到原生 hash：Rust 经 native registry 直接解析（无需存储部署态），C# 需 ContractManagement 中已有 deployed `ContractState`（genesis 才种入）。最小快照上表现为 **Rust HALT / C# FAULT**——这是**宿主解析路径的结构性差异**，读侧常量（symbol/decimals）两侧一致，但**未在 genesis 初始化态上对齐验证**。
- **风险**：`balanceOf`/`transfer` 等含原生存储读的方法，除解析差异外还依赖原生存储态，**完全未覆盖**。
- **待办**：在 C# 侧种入 NEO/GAS 的完整 `ContractState`（Nef+Manifest 序列化）后重新对齐。

### S3. 带签名交易的跨进程双向 roundtrip —— **未单独执行**
- **现状**：serialization 报告由"两侧编码字节恒等"**推导**出反向等价，但未把 C# 重编码字节**程序化喂回 Rust decode** 作为一次独立反向执行。
- **风险**：签名数据 = `network ‖ container.Hash`，两侧最小容器 hash 目前不一致（`GetRandom`/`CheckSig` 正路径因此无法对齐）。
- **待办**：增加"跨进程 C# 编码 → Rust 解码"独立反向执行步骤。

### S4. opcode price 表与 C# 交叉校验 —— **待办（TODO M-17）**
- **现状**：Coq 的 `opcode_price_full_256.v` 逐字证明了与 **Rust 表**一致（256 值机检），但 Rust 表本身标注 **TODO(M-17) 需与 C# 交叉校验**。
- **风险**：gas 记账依赖此表；若与 C# 有偏差，GasLeft/费用收敛会偏离，影响区块接受一致性。
- **待办**：完成 Rust 表 ↔ C# 参考表的交叉校验并登记结果。

### S5. Coq 9 个未通过模型 —— **明确未证**
- `abi_encoding`、`crypto_hash`、`mempool_validation`、`multisig_depth`、`multisig_validation`、`parallel_execution_safety`、`state_root`、`witness_script_hash`、`witness_validation` 均含 `Admitted`/`Axiom`/幽灵符号/空真命题，**不是机器可验证证明**（详见 `coverage-status-2026-09-18.md` §1）。
- 其中 `state_root`、`witness_*`、`multisig_*` 与**共识状态正确性**直接相关，属高优先级重建对象。

### S6. 完整区块状态一致（MPT/state root/原生余额） —— **无证据**
- G1-G4 均为**字节/VM 指令/部分 syscall 层**证据，**不等于**"完整区块状态一致"。
- **待办**：ApplicationEngine 状态转换差异测试、存储/MPT 状态差异测试、区块/交易序列化 roundtrip 重建。

---

## 4. 复现与证据追溯

- VM：`scripts/gen-vm-diff-vectors.py` + `vm_diff_runner` + C# runner + `scripts/vm-diff.py`（exit 0）。
- Serialization：`serialization_diff_runner` + `scripts/serdiff_compare.py`。
- Syscall：`scripts/gen-syscall-vectors.py` + `vm_syscall_runner` + `scripts/vm-diff.py`。
- Coq：`formal/ci/check_coq.py --all`（权威门禁）。
- 证据产物目录：`reports/formal/serialization-diff-2026-09-18/`、`reports/formal/syscall-diff-evidence/`。

---

**结论**：当前证据足以支撑 **阶段 A（跟随/观测节点）** 的部署，并作为阶段 B 的候选。**存储/原生合约价值转移（S1/S2）、完整区块状态一致（S6）、Coq 未通过模型（S5）构成阶段 B/C 的明确阻断项**，必须如实写入部署决策授权文件，不得声明"已生产就绪"。
