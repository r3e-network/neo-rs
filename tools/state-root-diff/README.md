# 端到端区块、状态根与执行收据差分工具

## 目标

从创世开始重放区块，通过真实 neo-rs 执行管线生成状态根，并与两个独立 Neo/C# RPC 上游逐块比较。若启用收据验证，还会比较系统与交易执行的 trigger、VM state、GAS、异常存在性、数量和顺序。本地成功生成状态根不等于跨实现一致；只有完成两侧差分后才能声明对应范围的 parity。

## 有限夹具处理器

`rust_state_processor.rs` 复用 `neo_validator::executor::NeoExecutor` 与 `neo-system` 的规范执行管线，不另建 MPT 实现，也不构造替代哈希。处理器要求输入为从高度 0 开始的连续区块，核对高度、哈希、大小、时间戳和交易数量，并拒绝执行失败、哈希不一致或零状态根。

```bash
cargo run -p state-root-diff -- \
  tools/state-root-diff/tests/fixtures/test_blocks.json \
  rust_state_roots.json
```

该示例输入是随仓库提交的创世到高度 10 的测试夹具（`tests/fixtures/test_blocks.json`）；`data/` 目录仅存放运行期下载产物，已被 .gitignore 忽略。

可选的第三个输出路径会导出原生合约存储，用于定位首个键值分歧：

```bash
cargo run -p state-root-diff -- \
  reports/mainnet/blocks-0-10.json \
  reports/mainnet/rust-state-roots-0-10.json \
  reports/mainnet/rust-state-0-10-native-storage.json
```

该命令适合有限夹具和差分实验；完整 MainNet 使用下面的持久化工具。

## 持久化 MainNet 全链验证器

`mainnet_state_verifier.rs` 使用 RocksDB 和 JSONL 审计文件，不把整条链载入内存。默认从两个公共 RPC 获取相同高度的证据：

1. 校验双方协议身份、network、执行参数和 hardfork；只允许公共节点本地 `msperblock` 出块间隔与发布配置不同。
2. 批量获取原始区块和状态根，要求双上游原始字节与状态根完全一致。
3. 严格反序列化区块，拒绝尾随字节、错误高度和错误 RPC id／缺失响应。
4. 通过 `NeoExecutor` 真实执行并逐块比较块哈希和状态根。
5. 默认每条审计记录单独 flush；显式启用 `--batch-durable` 时，先同步批边界 WAL，再 flush 该批审计。重启时要求数据库检查点、双上游检查点和审计末尾完全一致。
6. 数据库若已前进而审计未确认，重启 fail-closed，不静默跳过未确认区块。

```bash
cargo run --release -p state-root-diff --bin mainnet-state-verifier -- \
  --end 70000 --batch-size 2048
```

默认存储为 `reports/mainnet/full-validation/store`，审计为 `reports/mainnet/full-validation/verified.jsonl`，本次摘要为 `last-run.json`。可用 `--data-path` 和 `--audit-path` 选择独立证据目录；`--execution-mode canonical|auto|verify` 选择执行模式，默认 Canonical。

### 有界预取与性能剖析

默认启用仅下一批的有界预取，将下一批双上游 RPC 获取与当前批本地执行重叠。`--no-prefetch` 可关闭预取，用于同区段串行 A/B。最多保留当前批和下一批证据；当前批失败会 abort 并 await 下一批任务。区块执行、状态比较、存储提交和审计仍逐高度顺序进行，不跳过 VM、native hooks、MPT、状态根或收据验证。

`last-run.json` 的 `replay_profile` 分别记录 `upstream_seconds`（上游任务实际耗时）、`upstream_wait_seconds`（主循环等待墙钟耗时）、`decode_compare_seconds`、`local_execution_seconds`、`receipt_compare_seconds`、`durable_flush_seconds` 和 `audit_seconds`。`durable_flush_seconds` 只统计 `--batch-durable` 在批边界显式同步 WAL 的耗时，默认逐块 sync 时为 0。`overlapped_seconds` 表示阶段计时重叠估计；`unaccounted_seconds` 表示串行阶段之外的未归属耗时。预取时上游实际耗时可能与本地执行重叠，不能直接相加解释为墙钟耗时。

`--profile-execution` 默认关闭；开启后，`last-run.json` 另含 `execution_profile`，记录规范持久化管线的 snapshot open、transaction hash、OnPersist、transaction loop、transaction VM、PostPersist、participant precommit、store commit、runtime update 和 post-commit，以及状态根 change extraction 与 MPT。存储内部另记录 `store_writer_snapshot_open_seconds`、`store_change_application_seconds`、`store_write_batch_build_seconds`、`store_database_write_seconds` 和 `store_cache_invalidation_seconds`，分别对应 writer snapshot open、change application、WriteBatch build、database write 与 cache invalidation。

MPT 内部记录 `mpt_update_seconds`、`mpt_commit_seconds`、`mpt_root_hash_seconds`、`mpt_hash_seconds`、`mpt_serialization_seconds`、`mpt_hash256_seconds`、`mpt_node_cache_seconds`、`mpt_node_read_seconds`、`mpt_node_deserialize_seconds`、`mpt_node_encode_seconds` 和 `mpt_node_write_seconds`。update、commit、root hash 是包含式阶段，哈希、序列化和节点 I/O 还存在嵌套。

`transaction_vm_seconds` 已包含在 `transaction_loop_seconds` 中，`state_change_extraction_seconds` 与 `mpt_seconds` 已包含在 `participant_precommit_seconds` 中，上述五个存储子阶段全部包含在 `store_commit_seconds` 中，汇总时不得重复相加。profiler 仅观察墙钟耗时，不改变执行、提交顺序或错误处理。

MPT 提交复用解析时已有的 `initial_ref == 0` 不存在证据，通过 `put_with_hint` 避免新区块快照重复查询持久层；已有节点仍走原路径，快照状态冲突仍 fail-closed。高度 120,001–125,000 的两轮 A/B 中，本地执行从 4.6095 秒降至平均 3.9317 秒，MPT 下降 37.62%，节点写入下降 96.54%；两组各 125,001 条审计的高度、块哈希、状态根、交易数、收据数和收据标记全部一致。

StateService 另为每个 handler 实例持有独立的 65,536 项有界紧凑节点缓存，状态根关闭时不分配；独立链和存储不共享进程级条目，full-state trie 不按缓存引用计数裁剪历史根，重启从持久存储冷恢复。相同区段相对无缓存两轮均值，缓存两轮 node read 下降 33.14%、节点反序列化下降 99.14%、MPT 下降 16.04%，本地执行从 3.9317 秒降至 3.7115 秒（约 1.0593 倍）。

`--disable-mpt-node-cache` 可在独立 A/B 中显式绕过节点缓存，不清空条目或重置统计。`last-run.json` 的 `mpt_node_cache` 记录开关、容量、条目数、命中、未命中和命中率；`--profile-execution` 另记录 `process_memory` 的 initial/final/peak RSS 和采样数。RSS 是整个进程的观测值，包括 RocksDB、预取证据与 VM，不能全部归因于节点缓存。

高密度收据实验覆盖 **765,001–766,000**，四轮各 1,000 块、5,414 笔交易、7,414 条收据。缓存两轮相对无缓存均值：本地执行下降 **6.45%**、MPT 下降 **14.75%**、node read 下降 **33.93%**，平均命中率 **77.21906%**；平均最终／峰值 RSS 观测增加 **35.04 MiB（1.27%）**。四份完整 JSON 审计逐高度一致，详见 `reports/mainnet/replay-receipt-mpt-cache-ab-765001-766000.json`。

### 空块与简单转账快速路径 A/B

默认执行模式始终是 Canonical。空块和连续空块保留全部 native hooks、MPT、状态根和逐块提交；不会合并多个空块的状态转换或复用旧状态根。默认 interop 注册表按 pre-Faun／Faun 隔离共享，私有注册写时克隆；NEO committee 仅以完整存储字节为键缓存纯解码结果，不缓存可变链状态或旧状态根。

高度 **120,001–121,000** 的 1,000 块 clean A/B 中，committee 纯解码缓存使本地执行 **0.7393880 → 0.6365462 秒（-13.91%）**、PostPersist **0.1535920 → 0.0505641 秒（-67.08%）**，两份审计逐字节一致。`NEO_PERSIST_PROFILE=1` 可输出 committee／GAS 记录／mint／voter 的逐块细分诊断，但输出本身有开销，profiled 与 clean 结果不能交叉比较。

高密度 **765,001–766,000** 的 Canonical／Auto 同段实验各验证 5,414 笔交易和 7,414 条收据，完整审计一致。Auto 仅命中 **4 笔** direct，本地执行 **13.9411 秒**，Canonical 为 **12.2441 秒**；没有端到端提速证据，Auto 继续保持可选。连续 32 空块与单块 10 笔 GAS 已有 Criterion 基准，但没有旧同口径数据；单空块和单笔转账微基准噪声较大，不宣称稳定整块加速。

完整数据与限制见 `reports/mainnet/replay-empty-transfer-ab.json` 和 `reports/mainnet/replay-profiling.md`。这些限定区段不代表全 MainNet 已验证。

默认 batch size 为 32。实测报告及完整 A/B 范围见 `reports/mainnet/replay-profiling.md`，早期机器可读汇总为 `reports/mainnet/replay-profiling-summary.json`。公共 RPC 延迟存在波动，单次配对观测不是稳定加速保证。

### 离线批边界耐久同步

离线验证器可显式添加 `--batch-durable`，将 WAL 同步与审计确认对齐到批边界；应使用独立实验目录，不与正式证据目录混用：

```bash
cargo run --release -p state-root-diff --bin mainnet-state-verifier -- \
  --end 120000 --batch-size 2048 --batch-durable --profile-execution \
  --data-path reports/mainnet/replay-batch-durable/store \
  --audit-path reports/mainnet/replay-batch-durable/verified.jsonl
```

- 默认仍使用 `BatchCommitConfig::durable()` 逐块 sync，在线节点行为不变；该选项只供离线验证器显式选择。
- 每块仍原子写入，WAL 始终开启。批内写请求不单独 sync，每批完成后通过 `try_flush_durable()` 同步 WAL，错误向上传递。
- 只有 WAL 同步成功后才 flush 该批审计并报告进度；`RunSummary.batch_durable` 记录本次模式。
- 若中断发生在批边界之间，数据库可能已前进而审计尚未确认。重启会按既有水位规则 fail-closed，必须从一致的可信检查点恢复，不能静默跳过区块。

该模式只改变同步粒度，不跳过执行、MPT、状态根或收据验证。765,001–766,000 已补充高密度收据实验，但仍未通过机器掉电或故障注入验证。

### 从已有检查点开始新审计

独立 A/B 可复制已经验证的存储，再以空审计只记录后续新区块。此时必须显式添加 `--new-audit-at-checkpoint`；它要求本地已有检查点且审计缺失或为空，检查点块哈希和状态根仍由双上游复核。默认续跑规则不变，非空旧审计不能借该参数截断或改写。

```bash
cargo run --release -p state-root-diff --bin mainnet-state-verifier -- \
  --verify-receipts --end 766000 --batch-size 128 --batch-durable \
  --profile-execution --new-audit-at-checkpoint --disable-mpt-node-cache \
  --data-path reports/mainnet/replay-receipt-mpt-no-cache-765000/store \
  --audit-path reports/mainnet/replay-receipt-mpt-no-cache-765000/verified.jsonl
```

新审计只证明其实际包含的高度；例如 765,001–766,000 的收据审计不代表 0–765,000 的收据已经验证。

### 执行收据验证

加 `--verify-receipts` 后，工具同时查询块哈希和每个交易哈希的 `getapplicationlog`：当前 Neo/C# 上游的块级日志包含 OnPersist／PostPersist，交易级日志包含 Application。工具校验块哈希、交易哈希和触发器，按 `OnPersist → 各交易 Application → PostPersist` 重组，再比较双上游及本地收据。

```bash
cargo run --release -p state-root-diff --bin mainnet-state-verifier -- \
  --verify-receipts --end 20000 --batch-size 512 \
  --data-path reports/mainnet/full-validation-receipts-v2/store \
  --audit-path reports/mainnet/full-validation-receipts-v2/verified.jsonl
```

batch 512 已在高度 18,001–20,000 的 A/B 和正式目录 16,001–20,000 的续跑中验证；这些不是高交易密度区段，不能把 512 当作所有历史区段的通用最佳值。大批量会增加 `getapplicationlog` 调用数和响应大小，仍受 `--max-response-bytes` 限制，后续应按交易数、请求数和响应字节自适应分块。

不能假定“收据数 = 交易数 + 2”后直接查询块日志；真实高度 105 已证明交易 Application 收据需要按交易哈希获取。带收据和不带收据的审计不能混合续跑。收据 parity 目前覆盖上述字段，不等同于完整 VM 指令 trace 或所有 JSON 通知／结果栈字段的逐字节比较。

## 当前真实验证范围

独立摘要生成时，双上游共同链高为 **14,778,998**；这是历史摘要水位。以下数字只代表已完成区段，**全 MainNet 尚未完成**。

| 验证活动 | 范围 | 区块 | 结果 |
|---|---:|---:|---|
| 正式 Canonical 原始块、哈希与状态根 | 0–120,000 | 120,001 | 全部一致 |
| 独立 Canonical 原始块、哈希与状态根 | 0–845,000 | 845,001 | 247,994 笔交易全部一致；正式水位未改 |
| 正式 Canonical 上述范围及执行收据 | 0–25,000 | 25,001 | 全部一致 |
| 正式收据范围内的交易 | 0–25,000 | 162 笔 | 全部一致 |
| 正式收据范围内的系统与交易执行 | 0–25,000 | 50,164 条 | 全部一致 |
| 高密度收据缓存 A/B | 765,001–766,000 | 四轮各 1,000 | 每轮 5,414 笔交易、7,414 条收据，完整审计一致 |
| 独立检查点连续收据 | 765,001–840,000 | 75,000 | 54,981 笔交易、204,981 条收据；与 Canonical 主审计交叉一致 |
| Canonical／Auto／Verify 三模式哈希与状态根 | 0–11,000 | 11,001 | 全部一致 |
| Verify 实际 direct／Canonical 隔离双执行 | 0–11,000 | 52 笔 | 无指纹或状态分歧 |

- 最新独立续跑与交叉复核：`reports/mainnet/mainnet-continuation-765001-845000.json`。
- 独立连续性与公共区段复核：`reports/mainnet/full-validation-audit-summary.json`。
- 三模式报告：`reports/mainnet/execution-mode-parity-0-11000.json`。
- Canonical 当前日志：`.replay-atomic-canonical-110001-120000.log`。
- 收据当前日志：`.replay-atomic-receipts-20001-25000.log`。
- 性能剖析：`reports/mainnet/replay-profiling.md` 与 `reports/mainnet/replay-profiling-summary.json`。
- 高度 105 块级／交易级日志行为：`reports/mainnet/receipt-height-105.json`。
- 早期 `full-validation-receipts` 在高度 105 停止是工具错误假设的失败现场，不作为 parity 证据；有效收据证据位于 `full-validation-receipts-v2`。
- Auto 在相同 MainNet 区段没有展示端到端加速，公共 RPC 与存储开销占主导；不得把 direct 命中次数写成同步提速。

## 完整差分流程

1. 使用持久化工具从创世连续执行，不在受污染或未确认的状态上继续。
2. 发现分歧时记录高度、区块哈希、本地／上游根或收据字段并停止。
3. 提取该高度的交易、执行收据和存储变化，定位首个差异。
4. 修复后从创世或明确可信的检查点重跑；收据范围与状态根范围分别记录。
5. 只有指定范围内每个高度及所请求的收据字段都一致，才能记录为该范围的 100% parity。

## 依赖与边界

- neo-validator、neo-system、neo-ledger、neo-rpc client 与 neo-storage 的真实执行／存储实现。
- 两个独立且协议配置匹配的 RPC 节点；历史 `getapplicationlog` 必须可用。
- 全链验证需要足够的运行时间、网络稳定性和磁盘容量。当前链高会继续增长，未执行、缺失日志、跳过区块和仅生成根均不计为完成。
- 全仓单元测试和本地基准不替代 MainNet 全历史、长时运行或部署环境验证。
