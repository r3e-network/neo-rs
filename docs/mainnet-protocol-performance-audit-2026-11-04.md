# Neo N3 Rust Full Node：协议一致性、主网执行与性能审计报告

**日期：** 2026-11-04  
**目标协议：** Neo N3 v3.10.2  
**参考实现：** Neo 官方 C# v3.10.2，审计时固定到 commit `b50b06fc`  
**网络：** Neo MainNet，magic `860833102`  
**工作区：** `D:\Git\neo-rs`  
**报告范围：** 协议正确性证据、主网状态根验证、重放与实网性能、已实施优化、剩余瓶颈和后续方案

> 本报告区分三类信息：**本次实测**、**历史版本数据**、**未验证方案**。不同高度、不同交易负载或不同二进制的数据，不作为严格同条件 A/B 加速比。工作区包含未提交修改，本报告不表示这些修改已经形成发布版本或完成独立安全审计。

---

## 1. 执行摘要

### 1.1 协议正确性结论

在本次实际执行的验证范围内，**未发现 Rust 节点与 Neo 官方参考状态根之间的分歧**：

| 验证项 | 本次实测结果 |
|---|---|
| 独立主网执行重放 | 连续覆盖 `0..=13,141,250`，共 **13,141,251 块**；逐块状态根一致 |
| 正式节点 ACC 与 P2P 执行 | 继续执行到 **14,601,720**；静态参考根和官方 RPC 强制校验未发现分歧 |
| 最终实网停止状态 | 到达停止高度后完整 flush，并正常关闭；当时官方 tip 为 `14,601,723`，差 **3 块** |
| 工作区回归 | **229 个目标，4,793 通过，0 失败，66 忽略** |
| Criterion 回归报告 | 10 个比较项，**0 个超过 20% 阈值的回归**；`mpt_build_10000` 有 `+15.15%` 趋势，需要继续观察 |

上述 4,793 项工作区回归是本轮提交子阶段 profiling 修改前的结果。修改后已补跑状态存储测试 **61 通过、0 失败**，以及重放配置测试 **10 通过、0 失败、3 忽略**；尚未因此重复执行全工作区回归。

这些证据支持“**已验证的主网历史范围内协议执行一致**”，但不能把任何有限测试表述为对所有未来区块、所有异常网络条件和所有部署环境的数学式“100% 正确”证明。

### 1.2 性能结论

- **新二进制、同区间、三轮交错 A/B：** `commit_interval=1` 与 `64` 的重放循环中位数分别为 `15.776 s` 和 `5.123 s`，减少 **67.53%**，约 **3.08×**；完整进程 wall time 中位数从 `26.177 s` 降至 `15.513 s`，减少 **40.74%**，约 **1.69×**。六轮均逐块校验 2,000 个状态根，`reference_gaps=0`。
- **提交瓶颈已定位到实际后端写入：** 逐块提交的 `durable_commit` 中位总计为 `9.757 s`，占重放循环约 **61.85%**；其中 Ledger 提交 `4.785 s`、StateRoot 后端提交 `4.683 s`。64 块批处理后三者分别降至 `215.730 ms`、`114.997 ms`、`95.799 ms`。批处理后 `execute_total` 中位总计约占重放循环 **68.47%**，下一阶段应优先优化执行与 MPT flush。
- **重交易四档矩阵：** 在 1,000 块、6,628 笔交易区间完成 `1/16/64/256` 三轮测量，12 次运行全部通过状态根校验。64 与 256 的 wall time 中位数分别为 `80.562 s` 和 `80.567 s`，基本相同；256 的重放循环、p99 和峰值 RSS 略优，但第 700 块恢复点需重放 188 块，64 为 60 块，任意退出点的重放上界分别为 255 和 63 块。当前建议 **64 作为均衡重放默认值，256 作为离线吞吐/尾延迟优先的候选**，不把任何一档称为所有环境的全局最优。

历史与补充结论：

1. **最严重的已定位瓶颈是 NeoToken 候选人存储扫描。** 基线区间每块平均 `233.459 ms`，其中 `on_persist` 为 `224.150 ms`，NeoToken 占 `223.904 ms`；单次 committee 计算曾达到 `6.98 s`。前缀扫描修复后，相近高度的另一个区间每块平均降至 `3.070 ms`，committee 通常约 `0.2–0.7 ms`。由于两个区间的交易数量不同，不能把两者直接宣称为严格同数据加速比。
2. **同一个 2,000 块区间上的 trie 批处理对照有效。** 重放循环从 `6.3244932 s` 降至 `2.6583214 s`，减少 **57.97%**，约 **2.38×**；两组均校验 2,000 个状态根，`reference_gaps=0`。
3. **持久化批处理消除了大量逐块提交成本。** 在同一个 250 块区间，`durable_commit` 总耗时从 `884.443 ms` 降至 `28.209 ms`，减少 **96.81%**；这是阶段耗时对照，不是完整端到端 wall time。
4. **当前剩余瓶颈已转移到交易执行和 MPT/committing。** 重交易样本中，`avg_committing_ms` 仍占平均块耗时约 **35%**；交易执行也是主要成本。
5. **实网 P2P 仍受下载、frontier 恢复和 RPC 波动影响。** 最近 19 个约 5,000 块分片的中位耗时为 **96.190 s**，中位吞吐 **51.897 blocks/s**；完整范围为 `78.524–155.520 s`，说明不能把单一最佳批次当作稳定吞吐。
6. **没有得到一个可审计的“当前全主网重放总耗时”。** 历史重放由分段任务和不同版本组成，部分 driver 日志没有可靠 wall time。本报告不猜测“现在全量需要几小时”，而给出建立统一端到端基准的具体方案。

---

## 2. 审计基线与证据等级

### 2.1 协议基线

- 官方版本：Neo v3.10.2。
- 官方参考 commit：`b50b06fc`。
- MainNet magic：`860833102`，即日志中的 `0x334f454e`。
- 正确性判断以实际区块执行后的状态根为核心，而不只依据序列化编译、单元测试或区块高度增长。

### 2.2 证据等级

| 等级 | 含义 | 本报告中的例子 |
|---|---|---|
| A：同区间直接对照 | 相同区块范围、相同校验要求，比较明确阶段或重放循环 | 2,000 块 trie 批处理；250 块 durable commit |
| B：本次实测但非严格 A/B | 不同高度、交易负载或运行阶段，只用于瓶颈和趋势分析 | 候选人修复前后 1,000 块区间；重交易区间 |
| C：历史材料 | 旧版本或旧日期的数据，不能代表当前二进制 | v0.19.0 的 `286 s → 118.6 s` |
| D：方案/目标 | 尚未实施或尚未完成同条件验证 | 乐观并行、进一步 RPC 缓存、PGO 主网训练收益 |

---

## 3. 协议正确性验证

### 3.1 独立执行重放

独立重放路径执行每块交易以及 `OnPersist`/`PostPersist`，从上一状态根推进 trie，并与官方参考根比较。覆盖范围为：

```text
0..=13,141,250
共 13,141,251 块
结果：逐块状态根一致，无参考根缺口
```

重放测试的最终断言不仅检查根值，还检查实际比较数量等于请求区间块数，避免“提前结束但未比较全部区块”被误报为成功。实现位于：

- `neo-core/tests/mainnet_contiguous_replay.rs`
- `scripts/run-contiguous-acc-replay.py`

### 3.2 正式节点 ACC 与 P2P 验证

正式节点通过 ACC 分段导入及 P2P 实网同步继续执行到 `14,601,720`。静态参考根覆盖范围内使用本地参考数据，超出静态范围后通过官方 RPC 获取参考根；发生不一致或无法取得必要参考根时，应按失败路径处理，而不是跳过校验继续运行。

最终实网日志：

- `logs/audit-current-p2p-stop-14601720-final-live.log`
- 第 361–363 行：处理块 `14,601,720`，获取对应官方参考根并到达停止高度。
- 第 366–369 行：storage flush 完成，actor system 与节点正常关闭。

该日志未出现状态根 mismatch、ERROR 或 panic。停止时的另一次官方 RPC 查询得到 tip `14,601,723`，因此最终高度差 3 块是主动停止位置，不是同步落后故障。

### 3.3 工作区回归

实际执行命令：

```bash
"$USERPROFILE/.cargo/bin/cargo.exe" test --workspace --locked --no-fail-fast
```

结果：

```text
229 个测试目标
4,793 passed
0 failed
66 ignored
```

原始日志：`logs/audit-current-workspace-regression-v3102-final-rerun.log`。

66 个忽略项分类如下：

| 分类 | 数量 | 解释 |
|---|---:|---|
| 文档示例 | 57 | rustdoc 中标记为 ignore 的示例，不是 57 个失败协议测试 |
| 压力/负载测试 | 5 | large blocks、maximum throughput、network/spike/sustained load |
| 环境相关性能断言 | 3 | wall-clock 性能断言，避免在普通测试中受主机噪声影响 |
| 需要官方 MainNet RPC | 1 | 实网参考根获取测试 |

因此，“0 失败”不等于所有压力和实网测试都在本次工作区命令中执行；主网状态验证由独立重放和正式节点运行另外覆盖。

### 3.4 本轮修复覆盖的主要区域

从当前工作区修改和已执行验证看，修复涉及：

- VM 引用计数、脚本验证、控制流、序列化与异常语义。
- Application Engine 的合约调用、存储、费用、事件和 syscall 行为。
- NeoToken、Policy、Ledger、ContractManagement、CryptoLib、StdLib 等原生合约。
- 区块验证、存储迭代、MPT/StateService 和 RocksDB 持久化。
- P2P 请求窗口、任务去重、持久化 frontier 恢复和关闭流程。
- 近 tip 官方参考根的可用高度限制、重试与失败关闭。

这些修改尚未执行 git commit；报告描述的是当前工作区状态。

### 3.5 验证边界

本次结论不自动覆盖：

- v3.10.2 之后的新协议版本或未来硬分叉。
- 所有恶意 P2P 对端、长时间分区、频繁重组和任意资源耗尽场景。
- 多进程、多验证人生产 dBFT 部署的全部故障组合。
- 独立安全审计、依赖漏洞审计和所有 ignored 压力测试。
- 未实际执行的未来区块状态一致性。

---

## 4. 性能测量方法与限制

### 4.1 测量来源

1. **节点内部 phase timing**：同一进程统计 `avg_block_ms`、`avg_on_persist_ms`、`avg_tx_execute_us`、`avg_committing_ms` 等。
2. **独立重放 profiling**：统计 `execute_total`、`trie_commit`、`durable_commit` 和 `replay_loop_total`。
3. **正式 P2P 分片 wall time**：从 `Node starting` 到 `NEO_LIVE_STOP_HEIGHT`，包含启动、下载、执行、参考根校验和停止判断。
4. **Criterion 微基准**：用于观察局部回归，不等价于主网端到端吞吐。

### 4.2 不可直接横比的情况

- 不同主网高度的交易数和合约构成不同。
- ACC、独立重放和 P2P 的 I/O 与网络路径不同。
- 冷缓存、热缓存、RocksDB 状态和系统负载可能不同。
- phase 总和不一定等于完整 wall time；网络等待、调度和未计时阶段可能缺失。
- P2P 分片吞吐包含网络条件，不应与离线 ACC 吞吐直接比较。

---

## 5. 瓶颈与优化实测

### 5.1 NeoToken 候选人扫描：首要历史瓶颈

#### 基线：证据等级 B

日志：`logs/perf-node-persist-1630001-1631000.log`

| 指标 | 结果 |
|---|---:|
| 区块 / 交易 | 1,000 / 1,102 |
| `avg_block_ms` | 233.4589733 |
| `avg_on_persist_ms` | 224.1495208 |
| `avg_on_persist_native_ms` | 224.0928916 |
| NeoToken 平均耗时 | 223.904 ms |
| 导入总耗时 | 245.690 s |
| 整体导入速率 | 4.070 blocks/s |

NeoToken 几乎解释了该区间全部 `on_persist` 成本，说明当时优化 VM opcode 或 RocksDB 提交都不是第一优先级。

#### 定点 profiling

日志：`logs/perf-node-candidates-1631085-1631126.log`

| 测量 | 第一次 | 第二次 |
|---|---:|---:|
| committee 总耗时 | 6,980.63 ms | 6,759.38 ms |
| 存储扫描 | 3,570.99 ms | 3,333.32 ms |
| blocked 账户相关耗时 | 3,409.40 ms | 3,426.02 ms |
| 扫描候选人条目 | 28 | 28 |

只有 28 个候选人却消耗秒级时间，表明问题在存储遍历/读取路径，而不是候选人排序算法本身。

#### 前缀修复后：证据等级 B

日志：`logs/perf-node-prefix-fix-1631127-1632126.log`

| 指标 | 结果 |
|---|---:|
| 区块 / 交易 | 1,000 / 540 |
| `avg_block_ms` | 3.0703721 |
| `avg_on_persist_ms` | 0.3973337 |
| `avg_on_persist_native_ms` | 0.3404714 |
| committee 常见耗时 | 约 0.2–0.7 ms |
| 导入总耗时 | 19.159 s |
| 整体导入速率 | 52.194 blocks/s |

**结论：** 候选人前缀扫描修复是组件级数量级优化，并消除了重放中的主要异常慢点。但两个 1,000 块区间高度和交易数不同，不能把 `52.194 / 4.070` 当作严格同数据加速比。

### 5.2 Trie 批处理：同区间直接对照

对照区间：`13,139,251..=13,141,250`，均为 2,000 块，`commit_interval=64`，均完成全部状态根校验且 `reference_gaps=0`。

日志：

- 对照：`logs/perf-replay-indexed-batched64-detail-13139251-13141250.log`
- 优化：`logs/perf-replay-trie-batch64-detail-13139251-13141250.log`

| 指标 | 逐块 trie commit | 批处理/复用 trie | 变化 |
|---|---:|---:|---:|
| `replay_loop_total` | 6.3244932 s | 2.6583214 s | **−57.97%，约 2.38×** |
| `execute_total` 总计 | 2,683.678 ms | 2,131.175 ms | −20.59% |
| `trie_commit` 总计 | 2,902.796 ms | 91.246 ms | −96.86% |
| `durable_commit` 总计 | 254.310 ms | 146.664 ms | −42.33% |
| `on_persist` 总计 | 653.855 ms | 496.535 ms | −24.06% |
| `post_persist` 总计 | 625.304 ms | 583.722 ms | −6.65% |

`trie_commit` 的计数从每块一次变为每 64 块及区间末尾一次，符合批处理设计。优化后的 `execute_total` 也有所下降，但本报告不把这部分全部归因于单一改动，因为缓存和运行时状态可能影响执行阶段。

### 5.3 Durable commit 批处理：同区间阶段对照

对照区间：`13,141,001..=13,141,250`，共 250 块。

日志：

- 逐块：`logs/perf-replay-perblock-newbinary-detail-13141001-13141250.log`
- 64 块批次：`logs/perf-replay-batched64-detail-13141001-13141250.log`

| 指标 | 逐块持久化 | 64 块批次 |
|---|---:|---:|
| `durable_commit` 调用数 | 250 | 4 |
| `durable_commit` 总耗时 | 884.443 ms | 28.209 ms |
| 单次平均 | 3.538 ms | 7.052 ms |
| 单次 p50 | 3.412 ms | 6.960 ms |
| `apply` 总耗时 | 31.579 ms | 24.881 ms |
| `on_persist` 总耗时 | 202.461 ms | 183.254 ms |

批处理单次提交更慢是预期的，因为每次处理更多数据；关键是总提交次数和总耗时显著下降。两个日志都记录 `checked=250`、`reference_gaps=0`，但没有可靠的完整进程 wall time，因此这里只声明阶段收益。

#### 5.3.1 新二进制三轮交错 A/B：完整 wall time 与提交子阶段

本轮使用同一个 RocksDB 重放二进制，SHA-256 为 `75a0ad29d0e74efc5049dfe96c16910f804f3b59c07f71ef160e0df3cfd3a75c`。固定区间 `13,139,251..=13,141,250`，交错顺序为 `1,64 / 64,1 / 1,64`。每轮从相同 StateRoot 和 Ledger 基线硬链接克隆独立数据库，原始 SST 共享、可写元数据隔离；不修改正式重放状态。每轮均执行并比较 2,000 个状态根，同时断言持久化 readback，最终结果均为 `checked=2000`、`reference_gaps=0`。

结果文件：`benchmark-results/replay-commit-ab-20261104-072344/result.json`。完整进程 wall time 包含测试进程启动、数据库打开、参考根读取、重放和最终 flush，不包含正式采样前的数据库克隆。

| 指标 | `commit_interval=1` 中位数 | 最小–最大 | 标准差 | `commit_interval=64` 中位数 | 最小–最大 | 标准差 |
|---|---:|---:|---:|---:|---:|---:|
| 完整进程 wall time | 26.176848 s | 25.612753–26.839439 s | 0.614002 s | 15.512758 s | 12.909161–16.584079 s | 1.889950 s |
| `replay_loop_total` | 15.775755 s | 14.860136–16.266440 s | 0.713771 s | 5.122667 s | 3.016945–5.170578 s | 1.229803 s |

中位数对照：重放循环减少 **67.53%**（约 **3.08×**），完整进程 wall time 减少 **40.74%**（约 **1.69×**）。64 块组仍有明显波动，尤其是第一次执行较快，因此不能只采用最佳的 `3.017 s` 作为稳定结果。

提交相关阶段的三轮中位总计如下：

| 阶段 | 逐块提交 | 64 块批处理 | 中位数变化 |
|---|---:|---:|---:|
| `durable_commit` | 9,756.637 ms | 215.730 ms | **−97.79%** |
| `ledger_commit` | 4,784.745 ms | 114.997 ms | **−97.60%** |
| `state_backend_commit` | 4,683.303 ms | 95.799 ms | **−97.95%** |
| `trie_commit` | 1,384.354 ms | 586.268 ms | −57.65% |
| `trie_flush` | 1,186.466 ms | 566.991 ms | −52.21% |
| `trie_cache_commit` | 56.990 ms | 15.221 ms | −73.29% |
| `state_root_readback` | 16.006 ms | 6.193 ms | −61.31% |

逐块组的 `durable_commit` 占重放循环约 **61.85%**，是本轮最大单项成本；其后端写入明显大于 trie cache 提交。批处理后，`execute_total` 中位总计占重放循环约 **68.47%**，瓶颈转向交易执行和 `trie_flush`。三轮执行阶段本身波动较大，不能把执行时间差异归因于提交间隔。

实现上，`StateSnapshot::commit()` 仍按原顺序执行 `stage_commit()` 与后端 commit；新增拆分仅用于计时。正式节点可用 `NEO_PERSIST_PROFILE` 输出 snapshot 获取、trie 更新、flush、根哈希、参考根、根记录 staging、trie cache commit 和后端 commit；独立重放则使用本节的 `NEO_REPLAY_PROFILE` 指标。两类日志来源不同，不混合计算。

复现入口：`scripts/benchmark-replay-commit-ab.py`。驱动遇到 0 个测试、非零返回码、缺少完整状态根校验标记或缺少总耗时都会失败，不将空跑或状态分歧纳入性能结果。

### 5.4 重交易区间：瓶颈迁移到执行和 committing

以下数据来自不同高度，证据等级 B，只用于分析成本构成：

| 日志/区间 | 交易数 | 平均块耗时 | committing | commit | committed | 导入速率 |
|---|---:|---:|---:|---:|---:|---:|
| heavy `1,692,127..=1,693,126` | 8,565 | 40.584 ms | 未单列 | 0.282 ms | 未单列 | 16.271 blocks/s |
| committing `1,693,127..=1,694,126` | 6,628 | 32.266 ms | 11.273 ms | 0.237 ms | 1.260 ms | 17.875 blocks/s |
| trie reuse `1,694,127..=1,695,126` | 4,698 | 24.933 ms | 8.783 ms | 0.190 ms | 1.073 ms | 20.464 blocks/s |

在后两个区间，`avg_committing_ms / avg_block_ms` 分别约为 **34.9%** 和 **35.2%**。同时，逐交易执行平均约 `2.57–2.73 ms`，说明当前主要优化目标应是：

1. 交易执行中的存储读取、snapshot/合约状态和反序列化成本。
2. committing 内的 MPT 节点处理、序列化和持久化路径。

不能仅根据这三个不同区间宣称 trie reuse 带来了固定百分比的总加速。

#### 5.4.1 重交易四档批次矩阵：wall time、p99 与 RSS

本轮固定 `1,693,127..=1,694,126`，共 1,000 块、6,628 笔交易，比较 `commit_interval=1/16/64/256`。同一二进制 SHA-256 为 `7a8e093632bc2517670d2cbaca7968e3c251124a10fd7361e4c78fd3e8ecde50`。三轮顺序分别为 `1,16,64,256`、`256,64,16,1`、`16,1,256,64`。每轮使用独立硬链接克隆数据库；12 次运行均完成 `checked=1000`、`reference_gaps=0`，并验证每块持久化 readback。

汇总：`benchmark-results/replay-heavy-matrix-summary.json`。wall time 是完整重放进程耗时，不包括采样前的数据库克隆；p50/p95/p99 合并每档三轮共 3,000 个 `block_total` 样本，采用最近秩分位数。RSS 来自 Windows 进程高水位计数器，表中为三轮峰值的中位数。

| 批次 | wall time 中位数 | wall time 范围 | 重放循环中位数 | 块 p50 / p95 / p99 | 峰值 RSS 中位数 |
|---:|---:|---:|---:|---:|---:|
| 1 | 108.785 s | 108.565–159.064 s | 97.712 s | 98.705 / 247.285 / 324.214 ms | 1,853.99 MiB |
| 16 | 88.549 s | 79.343–90.210 s | 76.246 s | 59.820 / 178.209 / 276.153 ms | 1,452.52 MiB |
| 64 | **80.562 s** | 71.478–85.490 s | 69.798 s | 57.120 / 157.423 / 257.900 ms | 1,356.31 MiB |
| 256 | 80.567 s | 66.108–83.917 s | **68.385 s** | 54.729 / 151.596 / **209.632 ms** | **1,306.83 MiB** |

64 相对逐块提交的 wall time 中位数减少 **25.94%**。256 相对 64 的重放循环再减少 **2.02%**、p99 减少 **18.72%**、峰值 RSS 减少 **3.65%**，但两者 wall time 中位数只差约 `0.005 s`，不能据此宣称 256 的端到端吞吐更高。

重交易区间的主要成本已经是交易执行：`txs` 的三轮中位总计在逐块、64、256 组分别约为 `65.656 s`、`63.432 s`、`63.816 s`。因此，轻交易区间中显著的持久化批处理收益不能原样外推到重交易区间；继续优化 VM/存储读取和 MPT flush 比单纯增大批次更重要。

#### 5.4.2 中断恢复：确定性重放边界与实测差值

恢复测试在第 700 块后通过测试专用 `NEO_REPLAY_TEST_CRASH_AFTER` 强制进程退出，不执行区间末尾提交或 flush。随后核实持久化高度，从 `durable_height + 1` 续跑；另用全新克隆执行相同尾段作为对照。所有续跑和对照均通过完整状态根校验。汇总：`benchmark-results/replay-heavy-recovery-summary.json`。

| 批次 | 重复次数 | 该恢复点重放未提交块 | 恢复尾段块数 | 续跑 wall time 中位数 | 正常尾段中位数 | 续跑−对照中位数（范围） |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 1 | 0 | 300 | 23.462 s | 23.770 s | −0.309 s（单次） |
| 16 | 1 | 12 | 312 | 20.959 s | 23.381 s | −2.422 s（单次） |
| 64 | 3 | 60 | 360 | 21.114 s | 25.458 s | −0.814 s（−4.425～−0.767 s） |
| 256 | 3 | 188 | 488 | 33.884 s | 25.914 s | +2.961 s（−0.116～+9.273 s） |

负差值不是“恢复比正常执行更快”，而是说明缓存和主机噪声大于可稳定分辨的恢复差值。跨档尾段长度不同，也不能直接比较续跑 wall time。最可靠的恢复指标是确定性工作量：第 700 块退出时，64 与 256 分别重放 60 和 188 块；任意退出点的未提交重放上界分别为 63 和 255 块。

**批次建议：** 对需要兼顾恢复、尾延迟和稳定性的重放，当前选择 **64** 更均衡；对离线全量重放且更重视循环耗时、p99 和 RSS 的场景，可选择 **256**，但没有端到端 wall time 的显著收益。该结论适用于本次重放路径和重交易样本；正式节点的生产提交配置仍需在正式路径复测后才能调整。

### 5.5 P2P 实网分片性能

使用 `scripts/summarize-p2p-runs.py` 从 JSON 日志提取每次完整分片的启动高度、停止高度和 wall time。

#### 全部 117 个完成的约 5,000 块分片

| 指标 | 结果 |
|---|---:|
| 分片数 | 117 |
| 耗时最小 / 中位 / p95 / 最大 | 33.204 / 131.136 / 207.951 / 228.166 s |
| 吞吐最小 / 中位 / 最大 | 21.848 / 38.067 / 137.753 blocks/s |

原始汇总：`benchmark-results/p2p-sharded-run-summary.txt`。其中包含恢复、补块和不同网络阶段，极值不能单独代表稳定性能。

#### 最近 19 个分片（高度约 14.50M–14.60M）

| 指标 | 结果 |
|---|---:|
| 每片块数 | 约 4,960–5,023 |
| 耗时最小 / 中位 / 最大 | 78.524 / 96.190 / 155.520 s |
| 吞吐最小 / 中位 / 最大 | 32.099 / 51.897 / 63.573 blocks/s |

原始汇总：`benchmark-results/p2p-sharded-recent-summary.txt`。

**解释：** 近期中位性能约 52 blocks/s，但网络、frontier 等待和参考根 RPC 会造成明显波动。不能把近期中位吞吐直接乘以全主网块数来推算完整重放时间。

### 5.6 Criterion 微基准回归

文件：`benchmark-results/regression-report.json`

- 比较项：10。
- 回归阈值：20%。
- 最小绝对效应：10 µs。
- 超过阈值的回归：0。
- `mpt_build_10000`：`+15.15%`，绝对增加约 `23.0605 ms`，未触发 gate，但应作为趋势观察项。

`scripts/bench-regression.sh` 记录了开发主机在代码不变时出现约 10%–16% 波动的历史，因此小百分比变化不能直接解释为真实代码回归。当前 gate 只选择 `mpt_state_root` 和 `native_contracts`，完整主网关键区间仍需单独的端到端性能门禁。

### 5.7 历史版本数据：不得当作本次实测

以下来自 `RELEASE_NOTES_v0.19.0.md`，属于旧版本材料：

- 6,001 块重交易区间：`286 s → 118.6 s`。
- 增大状态节点缓存后：`115.9 s`。
- `getContract`：约 `4.5 ms → 1.5 µs`。

这些结果说明历史优化方向有效，但不能替代当前 v3.10.2 工作区的同区间复测。`WAVE*`、`TIER3*`、旧部署报告中的目标值、占位实现或宣传性倍数均不作为本报告证据。

---

## 6. 已实施优化及其正确性约束

| 优化 | 实测/观察结果 | 必须保持的协议约束 |
|---|---|---|
| NeoToken 候选人前缀扫描 | 消除秒级 committee 慢点 | 候选人、票数、blocked 账户和排序语义不能改变 |
| 重放 trie 复用与批量 commit | 同区间重放循环约 2.38× | 每块仍计算并比较状态根，不能只在批次末尾校验 |
| RocksDB durable commit 批处理 | 250 块区间阶段总耗时下降 96.81% | 崩溃恢复、flush、快照读取和状态根持久化必须一致 |
| MPT 节点哈希/序列化复用 | 减少重复哈希和二次序列化 | 存储字节、引用计数和根哈希必须与官方一致 |
| 合约/存储读取缓存 | 减少重复反序列化和读取 | snapshot 隔离、写后读和区块顺序不能改变 |
| P2P 请求窗口与缓冲匹配 | 避免 10,000 块窗口撑爆 256 块缓冲并反复丢块 | 不能将缺口误判为完成，不能丢失 frontier 块 |
| P2P 分片、任务去重和 frontier recovery | 实网同步能从停顿中恢复并持续推进 | 重试必须幂等，不能重复执行或跳过状态转换 |
| ACC 按本地高度跳过已导入数据 | 避免恢复时从创世重新执行 | 跳过仅针对已持久化前缀，启动高度和状态根必须核对 |
| 官方参考根 32 块预取 | 减少 RPC 次数 | 预取不能超过远端 `localrootindex`，不能缓存或捏造未来根 |
| 参考根重试与失败关闭 | 6 次尝试，退避 `100/200/500/1000/2000 ms` | RPC 暂时失败可重试，最终缺失必须失败，不能静默跳过 |
| 正常关闭与完整 flush | 最终实网日志确认完整关闭 | 停止高度后仍需持久化全部已执行状态 |

### 关于“空块优化”

空块仍可能包含协议要求的原生合约 `OnPersist`/`PostPersist` 状态变化，并产生新的状态根。**不能因为没有用户交易就跳过执行或沿用上一状态根。** 当前安全优化方向是复用 trie/缓存、减少重复读取和批量持久化，而不是省略协议钩子。

### 关于“乐观并行”

当前没有证据支持无约束并行执行同一区块交易。Neo 的状态读取、写入、费用、事件和调用顺序具有依赖关系；盲目并行会破坏确定性。可优先实施的是：

- 下载、解码、静态验证与有序执行之间的流水化。
- 在保留严格提交顺序的前提下做预取和预计算。
- 若未来研究交易级乐观并行，必须做读写集检测、冲突回滚、确定性重放和逐块状态根 A/B；在证明前不得用于正式执行路径。

---

## 7. 当前剩余瓶颈与风险

### 7.1 交易执行成本

重交易区间仍显示较高的逐交易执行成本。需要继续拆分：

- storage snapshot 创建与查找。
- `ContractState`、脚本和 `StorageItem` 反序列化。
- native contract 调用与跨合约调用。
- VM 引用计数、栈深复制和脚本哈希。
- 状态读取缓存命中率与缓存失效频率。

### 7.2 MPT / committing / RocksDB

`avg_committing_ms` 在重交易节点样本中仍占约 35%。本轮重放子阶段已进一步确认：逐块提交时，Ledger 和 StateRoot 后端写入是主要成本；64 块批处理后，执行和 `trie_flush` 成为更大的剩余部分。

已记录重放峰值 RSS、逐块 p99 和中断恢复工作量；仍需继续拆分后端 write batch 构造、WAL、操作系统 I/O 和磁盘写入量。不能仅凭“committing 较慢”就继续增大 batch。重交易矩阵显示 256 的 p99/RSS 更低，但 wall time 与 64 基本相同且恢复重放更多，因此当前 64 是均衡重放默认值，256 是离线尾延迟优先候选，不代表所有主网区间和生产部署的全局最优值。

### 7.3 近 tip 官方 RPC 延迟

近 tip 时远端可预取根数量可能只有 1，32 块预取无法摊薄请求成本。最终日志中每个新区块仍需获取官方根，RPC 网络延迟会直接反映到同步速度。这里既有执行节点成本，也有审计校验成本；做性能对比时必须分别记录“带强制 RPC 校验”和“仅本地执行”的时间，但正式一致性验证不能关闭校验。

### 7.4 P2P 下载与执行耦合

分片日志显示 `recovering stalled persistence frontier`、任务数变化和突发 drain。说明仍需要观察：

- 下载领先执行的距离。
- frontier 缺块等待时间。
- 重复请求、丢块和超时原因。
- unverified cache 占用与 drain 速率。
- 执行慢导致反压，还是网络慢导致执行饥饿。

### 7.5 性能回归覆盖不足

Criterion 微基准不能代表完整主网。本轮已建立一个同区间、同二进制、三轮交错 A/B 入口，但尚未形成覆盖多种负载的完整性能矩阵，尤其缺少：

- 冷/热缓存分开测量，以及重交易、native/MPT 密集区间的同类 A/B。
- RSS、磁盘写入量、I/O 等待和 RPC 延迟。
- 全量重放的单一 wall time 与分段 checkpoint。
- ignored 压力测试的定期执行结果。

---

## 8. 后续优化方案（按优先级）

### P0：建立可信性能基准与正确性门禁

1. 固定至少三类主网区间：空块/轻交易、重交易、native/MPT 密集。
2. 每个实验使用相同区块、相同起始状态、相同二进制配置和相同缓存条件。
3. 至少交错执行 3 次 A/B，报告中位数、p95 和离散程度，不只报告最佳值。
4. 保存完整 wall time、phase timing、RSS、RocksDB 写入量、I/O 等待、RPC 请求数/延迟和参考根校验数量。
5. 每次性能实验必须同时输出 `checked=<expected>`、`reference_gaps=0`，任何 mismatch 都失败关闭。
6. 为全量重放保留单一 driver wall time 和分段 checkpoint，避免只有局部 phase 数据。

### P0：保持协议正确性不可回退

- 持续启用逐块状态根校验；批次提交不能变成批次才校验。
- 官方 RPC 不可用或参考根未发布时，不允许用“未来根”“空根”或上一根替代。
- 优化后重新执行主网连续区间、工作区测试和近 tip P2P 验证。
- 将 MPT `+15.15%` 趋势列入复测，而不是等待超过 20% 才处理。

### P1：拆分 committing，针对真实成本优化

已为正式节点和独立重放增加 committing 子阶段指标，并完成固定区间三轮 A/B。后续继续拆分序列化、write batch、WAL 与 flush，根据结果选择：

- 提高节点/只读快照复用率。
- 减少 dirty node 重复遍历和重复分配。
- 合并相同 key 的写入，减少无效写放大。
- 调整 batch 大小时同时测量吞吐、RSS、p99 和恢复时间。

### P1：降低交易执行中的存储和对象成本

- 对 snapshot、合约元数据、脚本解析和 `StorageItem` 做命中率和耗时 profiling。
- 优先复用只读对象与字节，避免在每个 syscall 重复反序列化。
- 缓存必须有明确的作用域和失效规则，不能跨 snapshot 污染写后读。
- 以重交易主网区间验证收益，不能只依赖微基准。

### P1：为官方参考根 RPC 增加可观测性和安全缓存

- 记录 `getstateheight` 与 `getstateroot` 的请求数、batch 大小、重试和延迟。
- 对已经成功获取且绑定高度的不可变历史根使用有界缓存。
- 近 tip 仍先检查 `localrootindex`；不得缓存未来高度，不得因性能原因放宽缺失根处理。

### P2：下载/解码与执行流水化

- 用有界队列将网络下载、区块解码/静态验证和严格有序执行解耦。
- 根据执行速度动态调节下载窗口，避免 unverified cache 反复溢出。
- 分别测量网络等待、队列等待和执行时间，确定 frontier 停顿的主因。
- 保持交易和区块状态转换顺序，不实施无依据的乐观并行。

### P2：把关键主网区间纳入性能回归门禁

- 保留 Criterion 作为组件级信号。
- 新增固定主网区间的端到端 benchmark，并将 wall time、MPT 趋势和内存纳入报告。
- 对主机噪声建立最小绝对效应和多次中位数规则，避免 gate 变成随机失败。

---

## 9. 建议的下一轮验收标准

| 项目 | 通过标准 |
|---|---|
| 协议状态 | 所有请求块均执行并比较，`reference_gaps=0`，无 mismatch |
| 工作区回归 | 0 failed；ignored 项单独分类，不用忽略项掩盖失败 |
| 同区间性能 | 至少 3 次交错 A/B，报告中位数和波动，不仅报告单次最佳 |
| 全量重放 | 保留单一 start/end wall time、最终高度和 checkpoint 总和 |
| P2P 恢复 | 无永久 frontier 停顿；重复请求有界；正常 flush/关闭 |
| 资源使用 | 同时记录 RSS、I/O 和 p95/p99，避免以内存或尾延迟换吞吐 |
| 近 tip 校验 | 不超过 `localrootindex`，RPC 最终失败时关闭，不跳过验证 |

---

## 10. 复现命令与证据索引

### 10.1 工作区测试

```bash
"$USERPROFILE/.cargo/bin/cargo.exe" test --workspace --locked --no-fail-fast
```

证据：`logs/audit-current-workspace-regression-v3102-final-rerun.log`

### 10.2 重放 phase 汇总

```bash
python scripts/summarize-replay-profile.py logs/perf-replay-perblock-newbinary-detail-13141001-13141250.log
python scripts/summarize-replay-profile.py logs/perf-replay-batched64-detail-13141001-13141250.log
python scripts/summarize-replay-profile.py logs/perf-replay-indexed-batched64-detail-13139251-13141250.log
python scripts/summarize-replay-profile.py logs/perf-replay-trie-batch64-detail-13139251-13141250.log
```

本轮三轮交错 A/B 复现：

```bash
CARGO_TARGET_DIR=target/replay-overlay "$USERPROFILE/.cargo/bin/cargo.exe" test -p neo-core --features rocksdb --test mainnet_contiguous_replay --locked --no-run
python scripts/benchmark-replay-commit-ab.py --binary target/replay-overlay/debug/deps/mainnet_contiguous_replay-5b54d4f2dcae8741.exe
```

每次正式运行会新建带时间戳的结果目录；已有目录不会被覆盖。若重新编译后测试二进制哈希文件名变化，应使用构建命令实际输出的新路径。

重交易矩阵与恢复入口：

```bash
python scripts/benchmark-replay-commit-matrix.py run --binary target/replay-overlay/debug/deps/mainnet_contiguous_replay-5b54d4f2dcae8741.exe --intervals 1,16,64,256
python scripts/benchmark-replay-commit-matrix.py recover --binary target/replay-overlay/debug/deps/mainnet_contiguous_replay-5b54d4f2dcae8741.exe --interval 64 --crash-after 700
python scripts/benchmark-replay-commit-matrix.py recover --binary target/replay-overlay/debug/deps/mainnet_contiguous_replay-5b54d4f2dcae8741.exe --interval 256 --crash-after 700
```

`run` 子命令记录 RSS、p50/p95/p99 和完整状态校验；`recover` 的非 70 返回码、持久化高度不符或续跑状态分歧都会使结果无效。`summarize` 子命令用于合并同一二进制的多轮矩阵，`scripts/summarize-replay-recovery.py` 用于汇总恢复结果。

#### 当前全量重放基准（进行中）

独立驱动 `scripts/run-contiguous-acc-replay.py` 现默认 `commit_interval=64`，支持独立 StateRoot/Ledger 输出、原子状态文件、逐分段 JSONL checkpoint、累计有效耗时和 `--resume`。状态文件绑定 schema、二进制 SHA-256、输出路径、批次和请求范围；配置不同的运行不能续跑。每段仍要求精确测试成功标记、请求块数和 `reference_gaps=0`，没有添加 `BENCHMARK_ONLY`，持久化高度防跳块断言保持启用。

首次从创世启动发现：空输出的 `local_root_index()` 会回退到只读参考库的高度 `1,528,989`，从而被防跳块断言拒绝。现已在输出库自身缺少本地游标时显式初始化高度 0；参考库只提供历史 trie/根读取，不再被误认为新输出的执行进度。修复后的配置测试为 **10 通过、0 失败、3 忽略**，当前重放二进制 SHA-256 为 `efe451c94e72f72da8c96220a4526189e9fd7013da0c8c9cbc7af995010e3ad5`。

新运行命令：

```bash
python scripts/run-contiguous-acc-replay.py --start 1 --end 13141250 --step 2000 --max-seconds 240 --state-output data/replay-state-mainnet-batch64 --ledger-output data/replay-ledger-mainnet-batch64 --test-binary target/replay-overlay/debug/deps/mainnet_contiguous_replay-5b54d4f2dcae8741.exe
```

后续使用相同配置续跑：

```bash
python scripts/run-contiguous-acc-replay.py --start 1 --end 13141250 --step 2000 --max-seconds 240 --resume --state-output data/replay-state-mainnet-batch64 --ledger-output data/replay-ledger-mainnet-batch64 --test-binary target/replay-overlay/debug/deps/mainnet_contiguous_replay-5b54d4f2dcae8741.exe
```

运行证据为 `data/replay-state-mainnet-batch64-status.json`、`data/replay-state-mainnet-batch64-checkpoints.jsonl` 和带持久化 `run_id=314680cec1d548f28f792f64fc55e4eb` 的 `logs/audit-current-*` 日志。截至本次报告补充，已从创世连续执行到 **68,000**，共 34 个成功分段，68,000 次状态根比较全部一致，`reference_gaps=0`，累计有效工作耗时 **493.659 秒**。`active_wall_seconds` 只累计各次调用的成功分段工作时长，不含调用之间的暂停；日历时长单独记录，不能将断点续跑描述为不间断单进程 wall time。目标 `13,141,250` 尚未完成，最终全量结论和总耗时必须等待最终 checkpoint。

### 10.3 P2P 分片汇总

```bash
python scripts/summarize-p2p-runs.py logs/audit-current-p2p-stop-14*-sharded.log --min-blocks 4500
python scripts/summarize-p2p-runs.py logs/audit-current-p2p-stop-145*-sharded.log --min-blocks 4500
```

输出：

- `benchmark-results/p2p-sharded-run-summary.txt`
- `benchmark-results/p2p-sharded-recent-summary.txt`

### 10.4 Criterion 回归

```bash
bash scripts/bench-regression.sh --gate
```

当前已保存结果：`benchmark-results/regression-report.json`。该命令是回归入口，不表示本报告生成时重新执行了完整 benchmark。

### 10.5 关键原始日志

| 主题 | 文件 |
|---|---|
| 候选人扫描基线 | `logs/perf-node-persist-1630001-1631000.log` |
| 候选人定点 profiling | `logs/perf-node-candidates-1631085-1631126.log` |
| 前缀修复后 | `logs/perf-node-prefix-fix-1631127-1632126.log` |
| 重交易基线 | `logs/perf-node-heavy-1692127-1693126.log` |
| committing 拆分 | `logs/perf-node-heavy-committing-1693127-1694126.log` |
| trie reuse | `logs/perf-node-trie-reuse-1694127-1695126.log` |
| 250 块逐块提交 | `logs/perf-replay-perblock-newbinary-detail-13141001-13141250.log` |
| 250 块批量提交 | `logs/perf-replay-batched64-detail-13141001-13141250.log` |
| 2,000 块对照 | `logs/perf-replay-indexed-batched64-detail-13139251-13141250.log` |
| 2,000 块 trie 批处理 | `logs/perf-replay-trie-batch64-detail-13139251-13141250.log` |
| 新二进制三轮交错 A/B | `benchmark-results/replay-commit-ab-20261104-072344/result.json` |
| 重交易四档批次矩阵 | `benchmark-results/replay-heavy-matrix-summary.json` |
| 中断恢复汇总 | `benchmark-results/replay-heavy-recovery-summary.json` |
| 最终实网停止 | `logs/audit-current-p2p-stop-14601720-final-live.log` |
| 工作区最终回归 | `logs/audit-current-workspace-regression-v3102-final-rerun.log` |

部分早期 `perf-replay-driver-{baseline,batched64,indexed-batched64,perblock-newbinary}-*.log` 只有匹配数量，没有可靠 wall time；本报告没有根据文件时间或印象补猜端到端耗时。

---

## 11. 最终判断

当前工作区已经具备很强的主网协议一致性证据：从创世到 `13,141,250` 的独立逐块重放一致，正式节点继续通过 ACC/P2P 执行到 `14,601,720`，并最终正常 flush/关闭。修改前工作区回归为 4,793 通过、0 失败；本轮 profiling 修改后的 targeted 测试、轻交易六轮 A/B、重交易十二轮矩阵及恢复续跑状态校验均通过。

性能方面，候选人前缀扫描、trie 复用/批处理、持久化批处理和 P2P frontier 修复解决了多个已确认的主要瓶颈。轻交易同二进制三轮交错 A/B 给出重放循环约 **3.08×**、完整进程 wall time 约 **1.69×** 的中位收益；重交易矩阵则表明批处理收益会被交易执行成本摊薄。64 是当前更均衡的重放默认值，256 仅作为离线循环耗时、p99 和 RSS 优先的候选。交易执行、MPT flush、P2P 波动和近 tip RPC 仍是下一轮重点；全主网重放总耗时还需要用统一基准重新测量，不能用旧版本或不同区间的数据拼接出结论。
