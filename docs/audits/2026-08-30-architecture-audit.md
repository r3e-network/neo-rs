# neo-rs 架构审计报告

- **审计日期**：2026-08-30
- **审计对象**：`neo-rs` @ `430ca408`（release v0.15.0，Neo N3 v3.10.1 协议基线）
- **方法**：全仓静态取证（Cargo.toml 依赖图逐边核对、模块 LOC 统计、re-export 链追踪、CI workflow 逐文件核对、测试分布 grep 统计）
- **范围**：crate 分层 / neo-core 内部结构 / 单一事实源 / 测试与 CI 架构

---

## 1. 总体评价

架构经过 0.15.0 的多轮抽离后，**crate 级分层基本健康**：无循环依赖、无向上依赖（`tests/tests/layer_boundary_tests.rs` 已机械化强制）、方向大体符合 Foundation → Protocol → Services → Application。主要结构性问题集中在：

1. **neo-core 巨石**（89k 行 / 487 文件，占全仓 101k 行生产代码的 88%）——smart_contract 34k + network 17k 两大域拥挤，oracle_service、tokens_tracker、wallets、state_service 等完整业务子系统仍内嵌其中。
2. **默认构建面与分层声明失真**——README 分层图缺 neo-vm/neo-config/neo-json；`layer_boundary_tests.rs` 的 5 层模型与 README 4 层模型不一致，且仍引用已删除的 `neo-cli`；`neo-vm` 在测试模型中根本不存在（`from_crate_name` 未匹配），意味着"VM 归属层"这一最重要不变量**未被测试锁定**。
3. **双协议预设源**——`neo-core::ProtocolSettings` 与 `neo-config::ProtocolSettings` 各自硬编码 MainNet/TestNet 委员会与运营参数（内容当前一致，结构上仍是双源）。
4. **测试覆盖两极分化**——neo-core 713 单测 + 33k 行集成测试 vs neo-p2p/neo-config/neo-consensus/neo-vm 各只有个位数到百位数单测、0 集成测试；被攻击面最大的 P2P 线协议只有 19 个单测。
5. **遗留半成品痕迹**——同名 .rs/目录双形态原生合约文件、`application_engine` 文件群 2.5k 行平铺在 smart_contract/ 顶层、`compatibility-v391.yml` workflow 仍以 v3.9.1 命名。

---

## 2. Crate 分层与依赖图

### 2.1 实际依赖边（逐边核对 Cargo.toml）

| 层 | crate | 内部依赖（含门控） |
|---|---|---|
| L0 Foundation | neo-primitives | 无 |
| L0 | neo-json | 无 |
| L0 | neo-config | 无 |
| L0 | neo-storage | neo-primitives |
| L0 | neo-io | neo-primitives（path 依赖） |
| L1 Crypto | neo-crypto | neo-primitives、neo-io |
| L2 Protocol | neo-vm | neo-primitives、neo-crypto |
| L2 | neo-core | neo-vm、neo-json、neo-p2p、neo-config、neo-primitives、neo-crypto、neo-storage（均 path/workspace；rocksdb 由 `full`/`rocksdb` feature 引入，`neo-core/Cargo.toml:124,136`） |
| L2 | neo-p2p | neo-primitives、neo-crypto、neo-io |
| L2 | neo-consensus | neo-primitives、neo-crypto、neo-io、neo-vm（**不依赖 neo-core**，共识经 `ConsensusEvent`/`BlockData` 事件边界与节点层解耦，`neo-consensus/src/lib.rs`、`service/types.rs:27`） |
| L3 Services | neo-rpc | 基础层常驻；`server` feature 拉入 neo-config/neo-core/neo-json/neo-vm（`neo-rpc/Cargo.toml` features 段） |
| L3 | neo-telemetry / neo-hsm / neo-tee | 无或仅 neo-crypto |
| L4 Application | neo-node | neo-core(runtime)、neo-consensus、neo-p2p、neo-crypto、neo-vm、neo-rpc(server)、neo-telemetry；tee/hsm 为 optional（`neo-node/Cargo.toml`） |

### 2.2 分层判定

- **无环、无向上依赖**：`test_no_circular_dependencies` / `test_no_upward_dependencies` 通过（`tests/tests/layer_boundary_tests.rs:198,236`）。
- **值得表扬的解耦**：
  - `neo-consensus` 与 `neo-core` 完全解耦，仅靠事件类型交互——这是全仓最干净的共识边界，共识 crate 可独立测试。
  - `neo-rpc` 的 server 重依赖走 feature 门控，client-only 构建不拖入 neo-core。
- **可疑但合理的边**：
  - `neo-vm → neo-crypto`（`neo-vm/Cargo.toml`）：VM 需要哈希/曲线做 syscall 验签。方向正确（L2→L1），但使 neo-vm 不再是"纯 VM"。备选方案是把 VM 需要的密码学接口抽为 trait 由宿主注入——成本高、收益有限，现状可接受。
  - `neo-crypto → neo-io`（`neo-crypto/Cargo.toml`）：crypto 依赖 io 层（序列化辅助）。C# 对齐需要 Serializable 支持，属可接受的技术债，但让 L1 依赖了 L0 的 io 成员而非仅 primitives——`layer_boundary_tests` 明确将其列为允许例外（`test_crypto_only_depends_on_layer_0` 的 allowed 数组）。
- **唯一架构性越层**：`neo-core → neo-p2p`（`neo-core/Cargo.toml`）。p2p payload（Transaction/Block/Witness）实质是核心协议类型，却物理上放在 p2p crate 中，再被 core 反向依赖。这是 C# `Neo.Network.P2P.Payloads` 命名空间的直译产物；方向无环但使 "p2p 是 core 的传输层" 的直觉失效。**中风险**：阻碍未来把 neo-core 拆小（拆出去的任何协议 crate 都要连带拖上 p2p）。

### 2.3 分层声明三处失真（需要修）

| 位置 | 问题 | 证据 |
|---|---|---|
| README 架构图 | 4 层图缺 neo-vm/neo-config/neo-json 的准确归属；`neo-vm` 标为 "Core Layer + VM compatibility" | `README.md:100-108` |
| layer_boundary_tests 模型 | 5 层模型：`from_crate_name` **未映射 neo-vm、neo-config 在 Foundation 但注释矛盾**（注释说 Layer 0 含 neo-config，代码匹配列表里 neo-config 在 strict_layer_0 之外，实际从 `from_crate_name` 匹配）；`neo-vm` 完全不在模型里 → neo-vm 的任何依赖变化不触发该测试 | `tests/tests/layer_boundary_tests.rs:9-15,33-46` |
| 失效引用 | 测试仍包含 `"neo-cli"`（已删除的 crate），README 仍描述 `neo-cli` 用法 | `layer_boundary_tests.rs:46`、`README.md:198,215` |

### 2.4 默认构建面

- `default-members` = 11 个运行时 crate，**不含** tee/hsm/telemetry/tests/benches（`Cargo.toml:69-81`）——日常 `cargo build/test` 快，可接受。
- `fuzz` 独立 workspace（exclude），依赖经 path+version 双写（`fuzz/Cargo.toml:15-18`）——每次发版需手动同步（本次 0.15.0 已同步），长期应改 workspace 依赖。
- **风险**：`neo-node` 不在 CI 的 Windows 本机可编译（rocksdb），且 CI 测试跑全 workspace——本机验证与 CI 验证面不同，见 §5。

---

## 3. neo-core 内部结构

### 3.1 域规模（前 6 名，共 89,153 行 / 487 文件）

| 域 | 行数 | 文件数 | 占比 |
|---|---|---|---|
| smart_contract/ | 34,032 | 151 | 38% |
| network/ | 17,064 | 74 | 19% |
| ledger/ | 6,023 | 26 | 7% |
| oracle_service/ | 5,294 | 90 | 6% |
| state_service/ | 4,380 | 19 | 5% |
| neo_system/ | 3,936 | 16 | 4% |

**问题**：neo-core 实际是 8+ 个完整子系统（含 oracle 客户端、NFT/NEP17 token 追踪器、钱包 NEP6、actor 框架、状态服务）塞在一个 crate 里。任何一域改动都重编整个 89k 行 crate。

### 3.2 门面/shim 层（迁移后兼容面，当前状态良好）

| shim | 行数 | 转发到 | 守卫 |
|---|---|---|---|
| `neo-core/src/neo_vm.rs` | 13 | `pub use neo_vm::*` | no_local 守卫测试锁定为纯 glob 门面 |
| `neo-core/src/script_builder.rs` | 2 | neo_vm::script_builder | 同上 |
| `neo-core/src/smart_contract/binary_serializer.rs` | 2 | neo_vm::binary_serializer | 同上 |
| `neo-core/src/big_decimal.rs` | 2 | neo_primitives::big_decimal | — |
| `neo-core/src/smart_contract/call_flags.rs` | 2 | neo_primitives::call_flags | 同上 |
| `neo-core/src/script_validation.rs` | 76 | neo_vm（parse/validate re-export） | p2p_validation 守卫测试 |

门面策略本身执行得干净（121 个守卫测试锁定 shim 不膨胀）。**遗留**：`neo-core/src/rpc/exception.rs` 是对 `neo_primitives::RpcException` 的 2 行转发，而 neo-core 顶层另有 `rpc/mod.rs` 仅 8 行——整个 `neo-core/src/rpc/` 目录可并入 lib.rs 直接 re-export。

### 3.3 原生合约 .rs/同名目录双形态（组织不一致）

`smart_contract/native/` 下 5 个合约同时存在 `X.rs` 与 `X/` 目录：

| 合约 | .rs 文件角色 | 目录角色 |
|---|---|---|
| notary.rs + notary/ | 完整实现（340+ 行实现 + deposit/verification 逻辑在两侧） | 子模块（deposit.rs 等） |
| oracle_contract.rs + oracle_contract/ | 顶层 metadata/methods | verification/config/storage 子模块 |
| role_management.rs + role_management/ | 完整实现 | storage 子模块 |
| std_lib.rs + std_lib/ | 完整实现 | helpers/strings 子模块 |
| ledger_contract.rs + ledger_contract/ | metadata/prefix 常量 | keys/storage/native_impl 子模块 |

对照整齐的对照组：`policy_contract/`、`contract_management/`、`neo_token/`、`gas_token/`、`crypto_lib/` 是纯目录形态。**建议**：将 5 个 .rs 主体移入各自目录的 `mod.rs` 或 `native_impl.rs`，统一为目录形态（纯机械、低风险、消除"实现在哪"的认知成本）。

### 3.4 ApplicationEngine 文件群

- 平铺文件 `neo-core/src/smart_contract/application_engine*.rs` 共 8 个、2,529 行（application_engine.rs、application_engine_contract.rs、application_engine_crypto.rs、application_engine_helper.rs 等）。
- 目录 `smart_contract/application_engine/` 共 10 个文件、3,810 行（state.rs、storage_low_level.rs、witness_and_misc.rs 等）。
- 同一引擎的两半分别以"前缀平铺 + 子目录"两种风格组织。**建议**：平铺 8 文件并入子目录（与 3.3 同类机械归一）。

### 3.5 大文件与死代码

- 非测试最大文件集中在 `neo-vm/src/stack_item/stack_item.rs`（1190）、`neo-core/src/state_service/…`、`wallets/helper.rs`（859）、`manifest/contract_manifest.rs`（834）——均属复杂度内聚，可暂不动。
- `#[allow(dead_code)]` 31 处（neo-core/neo-vm/neo-rpc）；全仓 TODO/FIXME 仅 1 处——代码卫生良好。
- `neo-core/src/monitoring/`（717 行）挂在 `monitoring` feature 后；`telemetry/`（796 行）与独立 `neo-telemetry` crate 职责相邻，是未来合并候选。

---

## 4. 单一事实源记分卡

| 项 | 状态 | 证据与风险 |
|---|---|---|
| UInt160/256/BigDecimal/CallFlags/Hardfork 枚举 | ✅ 单源（neo-primitives），消费方走 shim | 硬分叉激活高度已收敛到 neo-config（本次 0.15.0 完成），全仓无非测试字面量散落（grep 12020000/8800000 于生产代码零命中） |
| StorageKey/StorageItem | ✅ 单源（neo-storage），neo-core 经 `persistence/mod.rs:3-12` re-export | 链路：neo-storage 实现 → `neo-core::persistence` → `neo-core::smart_contract::storage_key`（2 行 shim）。三层 re-export 是迁移残留，可在守卫测试保护下扁平化 |
| WitnessRule/WitnessCondition | ✅ 单源（neo-io 实现，629 行），neo-core `witness_rule.rs` 是声明式门面 | grep 证实 neo-core 内 14 处 `crate::witness_rule` 全部指向门面；**非重复实现** |
| 协议预设（委员会/参数） | ⚠️ **双源** | `neo-core/src/protocol_settings.rs:96-145`（21 公钥+seeds+ms/tx 硬编码）与 `neo-config/src/protocol.rs:191-245` 各一份；今日逐字段 diff **内容完全一致**（委员会 25 行 diff 为空）。但无一致性测试锁定——任一侧改动（如再次同步 Gorgon 参数）都会静默分叉。**建议**：`ProtocolSettings::mainnet()` 改为从 `neo_config::ProtocolSettings::mainnet()` 构造（与 HardforkManager 同法），或加逐字段等价守卫测试 |
| Ledger/Transaction payload | ⚠️ 分工依赖直觉 | 权威类型在 `neo-core/src/network/p2p/payloads/`，`neo-primitives/src/blockchain/` 只放 BlockLike 等 trait——分工成立但 "network/p2p/payloads 放核心协议类型" 与 crate 名义职责相悖（同 §2.2 越层问题） |
| ConsensusPayload/ExtensiblePayload | ✅ 单源（neo-core payloads），neo-consensus 只定义消息与事件 | 边界干净 |

---

## 5. 测试、CI 与发布架构

### 5.1 测试分布（unit = src 内 #[test]；int = tests/ 目录）

| crate | unit | int files | int LOC | 评价 |
|---|---|---|---|---|
| neo-core | 713 | 106 | 33,493 | 主力，含 13 个主网块 state-root 回放 |
| neo-rpc | 287 | 6 | 826 | server 侧测试充分 |
| neo-primitives | 264 | 1 | 245 | 充分 |
| neo-storage | 160 | 0 | 0 | 可接受（单测为主） |
| **neo-vm** | 119 | 0 | 0 | **VM 是共识核心却无集成测试**；单测数对 25.6k 行偏薄 |
| neo-crypto | 124 | 1 | 178 | 充分 |
| neo-consensus | 58 | 0 | 0 | 中等；service 集成场景靠根 tests/ 补 |
| **neo-p2p** | 19 | 0 | 0 | **P2P 线协议几乎裸奔**——握手/消息/压缩/超时仅 19 个单测，无握手集成测试 |
| **neo-config** | 14 | 0 | 0 | protocol.rs 承载网络预设却只有 14 测 |
| neo-node | 38 | 1 | 113 | 启动/配置测试在但 daemon 级 e2e 缺（依赖 rocksdb 环境） |

根 `tests/`（neo-tests，7,363 行 / 10 文件）定位清晰：121 例架构守卫 + e2e + 混沌 + 层边界。**它是事实上的"架构 CI"**，但守卫只覆盖 neo-vm 抽离这一个主题；分层模型本身失真（§2.3）。

### 5.2 忽略测试

全仓 `#[ignore]` 约 16 处，理由分布：
- 11 处：`requires local mainnet full-state data`（state-root 回放，合理的环境依赖，但**没有任何 CI job 准备这些数据**——高价值测试在 CI 永远跳过）。
- 2 处："pre-existing issue"（block validation system context）——未跟踪的悬案。
- 2 处："test vectors not populated"（genesis/block vectors 为空数组）——向量基建未完成。
- 1 处：consensus view test "needs investigation"。

### 5.3 CI 差距（承诺 vs 实际）

| 项 | docs/RELEASE.md 承诺 | CI 实际（.github/workflows/ci.yml） |
|---|---|---|
| fmt | `cargo fmt --all` | ✅ `cargo fmt --all --check`（ci.yml:27） |
| clippy `-D warnings` | 要求 | ⚠️ 仅 `-D clippy::all`（ci.yml:62）——`missing_docs` 等 rustc lint 不致命，neo-primitives 43 条 missing_docs 存活。要么升格为 `-D warnings`（需先清 43+ 条文档债），要么把 RELEASE.md 的承诺改准 |
| test | `cargo test --workspace` | ✅ nextest 全 workspace + doc tests（ci.yml:102-106），Linux 环境**覆盖 neo-node/rocksdb**（补上本机盲区，好） |
| 协议一致性 | — | ⚠️ `compatibility-v391.yml`：名称、脚本（`validate-v391-consistency.sh`）、路径过滤全部锚定 v3.9.1；基线已升 v3.10.1，**workflow 与脚本需重命名/升级**，否则每 12 小时跑的是过期基线 |
| 状态回放 | — | ❌ 主网 state-root 回放测试所需 DB 无 CI job 准备 |

### 5.4 质量信号

- `warn(missing_docs)` 开启于 6 个 crate（primitives/crypto/io/json/vm/consensus）→ clippy `-D warnings` 若升格，第一优先清 neo-primitives。
- scripts/ 56 个脚本中仅 1 个被 CI 引用（validate-v391-consistency.sh）；其余为运维/对拍工具，无清单文档说明哪些仍在用——**孤儿脚本面大**。
- 文档残留：`docs/audits/`（v3.9.1 时代审计）与 `openspec/changes/archive/` 为历史档案可保留；`docs/DEPLOYMENT/ARCHITECTURE/SECURITY/COMPLETION-REPORT/deployment-report` 已随 v0.15.0 同步。

---

## 6. 结论与建议（按收益排序）

> **落地状态（第二轮优化后）**：建议 1–4 已完成并新增边界测试；建议 5 的 compatibility workflow/script 已升级至 v3.10.1 并修复可复现性/报告归档问题，state-root 可选 CI 与 clippy 文档债务仍待后续处理；建议 6 保留为长期拆分路线。

1. **【已完成】修复分层声明三处失真**：`layer_boundary_tests.rs` 已加入 neo-vm/neo-hsm、删除 neo-cli 映射，并要求所有生产 crate 必须被识别；README 架构图与当前 crate 布局已对齐。
2. **【已完成】协议预设收敛为单源**：`ProtocolSettings::mainnet()/testnet()` 已从 `neo_config::ProtocolSettings` 转换，新增逐字段等价测试并保留 core 安全 `default()` 语义。
3. **【已完成】ApplicationEngine 文件归一**：7 个平铺 `application_engine_*.rs` 已迁入 `smart_contract/application_engine/`，旧路径保留兼容 alias；原生合约入口保持行为不变。
4. **【已完成】补 P2P/VM 集成测试**：新增 `neo-vm/tests/protocol_integration.rs`（3 例）与 `neo-p2p/tests/protocol_roundtrip.rs`（4 例），覆盖公开 API、限制边界、消息/标志/库存类型。
5. **【部分完成】CI 收尾**：compatibility workflow/script 已升级为 v3.10.1 命名、参数与报告路径；state-root 可选 CI 与 43 条 missing_docs 清理仍是后续债务。
6. **【低】长期拆分路线**（保留）：neo-core 拆出 oracle_service / tokens_tracker / wallets / state_service；p2p payloads 类型迁往独立协议层 crate，消除 core→p2p 越层。

---

## 7. 第二轮架构优化落地记录（2026-08-30）

### 7.1 已落地

- **分层模型**：`tests/tests/layer_boundary_tests.rs` 纳入 `neo-vm`、`neo-hsm`，删除 `neo-cli`，并要求所有生产 workspace crate 必须被 `Layer::from_crate_name` 识别；层边界测试 11/11 通过。
- **ProtocolSettings 单源**：`neo-core::ProtocolSettings::mainnet/testnet` 通过显式 `from_config` 转换使用 `neo-config` 预设；对 u64→u32、u32→i32、i64→u64 使用 checked conversion；新增 `protocol_settings_source_tests.rs`（3 例）。
- **ApplicationEngine 归一**：7 个 `application_engine_*.rs` 已迁入 `smart_contract/application_engine/`，旧路径通过 alias 保持兼容；`neo-core` 全部目标测试通过。
- **VM/P2P 集成测试**：新增 `neo-vm/tests/protocol_integration.rs`（3 例）和 `neo-p2p/tests/protocol_roundtrip.rs`（4 例）。
- **Persistence 边界守卫**：新增 `tests/tests/persistence_boundary_tests.rs`（1 例），锁定 `neo-storage` 为 `StorageKey` 唯一实现体，core 仅保留 re-export shim。
- **兼容性流程**：workflow/script 从 v391 重命名到 v3101；artifact path 修正为单行 glob；增加最小权限和基础层触发路径；主网 3000ms 参数修正；policy mismatch 默认关闭；execution-specs 要求显式不可变 ref 并记录实际 commit。
- **当前文档**：README、架构文档、活动 OpenSpec、CLI/Operations/Monitoring 文档统一到 neo-rs v0.15.0 / Neo N3 v3.10.1；历史 audit/archive 保持原样。
- **lint 可见性**：移除 `neo-core`、`neo-vm` 的 crate 级 `allow(missing_docs)`，改为 warning，防止新增 API 静默缺文档。
- **占位测试治理**：空向量测试与 NeoToken parity 测试明确标注 v3.10.1 fixture 未 provision，不将占位断言计入协议覆盖率。

### 7.2 验证结果

```text
cargo check --workspace --exclude neo-node --all-targets     PASS
cargo test --workspace --exclude neo-node --no-fail-fast    2752 passed / 0 failed / 49 ignored
layer_boundary_tests                                      11 passed
no_local_neo_vm_dependency                                121 passed
persistence_boundary_tests                                  1 passed
neo-vm protocol_integration                                3 passed
neo-p2p protocol_roundtrip                                  4 passed
```

### 7.3 明确剩余债务

- `neo-node` 的完整 Windows 构建仍受 LLVM-MinGW 编译 `librocksdb-sys` 缺少 MSVC `FILE_ID_INFO` 头文件影响；Linux CI 才能覆盖该构建面。
- compatibility workflow 需要仓库变量 `NEO_EXECUTION_SPECS_REF` 提供已审核的 v3.10.1 execution-specs tag/commit；未提供时脚本现在会 fail closed，不再漂移到远端 `main`。
- state-root replay 测试仍需要外部 full-state fixture；应由独立 nightly/replay job 提供，不能在普通 PR CI 中伪造通过。
- `neo-primitives` 等 crate 仍有既有 public API missing-doc warning；本轮只恢复可见性，未将历史文档债务混入协议重构。
- 原生合约 `.rs`/同名目录双形态和 neo-core 大型子系统尚未整体拆 crate；这是下一阶段的高风险结构重构，不与本轮低风险边界治理混合。

## 附录：本审计的方法学证据

- 依赖边：逐个 `Cargo.toml` `[dependencies]/[features]` 段核对（含 optional）。
- 双源判定：`grep -oE '"[0-9a-f]{66}"'` 提取两侧委员会列表做 diff（结果为空 = 内容一致）；参数以 `grep -nE 'ms_per_block|max_transactions_per_block|MAINNET_MAGIC'` 两侧对照。
- 门面判定：对每个嫌疑文件 `head` + 全仓 `use crate::witness_rule` 等 grep 引用计数。
- 测试统计：`grep -rc '#\[test\]'` 按 crate 汇总 + `find tests/ -name '*.rs' | wc`。
- CI：`.github/workflows/{ci,compatibility-v3101,fuzz,release}.yml` 逐文件核对触发条件与步骤。
