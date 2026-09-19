# Coq ↔ TLA+ 形式化桥：规划与现状评估（Krilla / 类型桥）

**日期**：2026-09-19
**范围**：Coq 模型（`formal/coqlib/`，权威门禁 `formal/ci/check_coq.py --all`）与 TLA+ 模型（`formal/consensus/model/`，TLC 有界验证）之间的类型/语义对应。
**目标**（来自原计划）：*"Krilla bridge planning (Coq ↔ TLA+ correspondence)"*。
**原则**：诚实评估；对任何无法在本环境实测确认的工具能力标注"需实测/待验证"，不做超出现状的宣称。

---

## 0. 执行摘要（TL;DR）

1. **Krilla（Rust→Coq 类型导出）在本环境不可行，且大概率不存在**。经 crates.io 与 GitHub 检索，名为 `krilla` 的唯一真实制品是一个**无关的 PDF 生成库**（LaurenzV/krilla），没有任何公开的"Rust→Coq 类型导出"工具叫 Krilla。原计划所引用的 `cargo krilla-export` 作业在 CI 中是**假作业**（只 echo 成功），现已被 fail-closed 重写移除（见 `.github/workflows/formal-verification.yml` 注释与 `reports/formal/coverage-status-2026-09-18.md` §2.3）。因此"用 Krilla 搭桥"**不应作为任何里程碑的依赖**。
2. **推荐方案（低侵入、稳健、不引入外部工具链）**：
   - **主方案**：手工维护的 **Coq↔TLA+ 映射表**——把已通过的 Coq 模型结论与 TLA+ 不变式逐一对应，以文档+机器可查的对应文件登记，两边共享同一组语义常量。
   - **强化**：为少量**数值/常量语义**（如 dBFT 的 `f=(n-1)/3`、`M=n-f`，varint 的标记与长度分档 `{0xFD,0xFE,0xFF}→{1,3,5,9}`）建立**单一事实来源生成器**，同一份 JSON 输入同时生成一段 Coq 模块与一段 TLA+ 常量声明，给出**机检一致点**。
3. **对应层级**：只主张两套语义在**"协议规格/抽象模型"层一致**，**不主张程序级精化**（Coq 不是对 Rust 可执行文件的精化证明，这一点 `coverage-status` 已明确）。
4. **诚实验收标准**：见 §7。能证明"某一不变量/语义在 Coq 与 TLA+ 两侧表述一致"才算桥完成；仅整理映射表/路线图只算规划。

---

## 1. 现状评估（只读核查结果）

### 1.1 Coq 侧（`formal/coqlib/`，权威门禁）

权威判定为 `formal/ci/check_coq.py --all`：在隔离临时目录对每个 `*.v` 独立执行 `coqc -Q coqlib CoqLib`，仅当编译退出 0 才跑 `coqchk -silent`；编译/内核任一失败即**不通过**，且不复用旧 `.vo`，并统计 `Admitted/admit/Axiom/Parameter` 标记。工具链 Coq 8.18.0 / OCaml 4.14.1（WSL Ubuntu）。

- 全量 **34 个 Coq 模型：25 通过 / 9 未通过**（详见 `reports/formal/coverage-status-2026-09-18.md`）。
- **与桥相关的已通过模型**（阅读确认，均为无公理/admit 的诚实模型，且头注释明示"抽象模型、非 Rust 精化"）：
  - `rw_set_minimal.v`：并行 RwSet 冲突检测（`conflicts`、`member`，`conflicts_spec`、`no_missed_conflict`）。
  - `consensus_roundtrip_refinement.v`：per-validator 消息池 `store_prepare`/`store_commit`，证明 round-trip 恒等与非干扰。
  - `mempool_validation_refinement.v`：容量 `enforce_capacity = firstn cap`、`mempool_never_exceeds_capacity`（**非空真**）、去重、NoDup 不变式、`better` 费用优先级。
  - `commit_message_refinement.v`（dBFT Commit）：`CommitMessage`（`block_index:u32→nat, view_number:u8→nat, validator_index:u8→nat, signature:list nat`），`validate_commit ⟺ length sig = 64 ∧ all bytes ≤255`，`serialize_commit = signature`；明确不对 ECDSA/secp256r1 作声明。
  - `varint_encoding_refinement.v`：`encoded_len`/`write_var_int`/`read_var_int_prefix`，证明小值/u16/u32 长度 =1/3/5，**u64/9 字节未证**（`read_var_int_prefix` 对 u64 返回 None，README 明示）。
  - `change_view/viewchange/prepare_request/prepare_response/commit_response/oracle_response/block_validation_roundtrip/serialization_roundtrip` 等 Track5 重建模型。
- **9 个未通过**均带 `Admitted`/`Axiom`（`abi_encoding`、`crypto_hash`、`mempool_validation` 旧版、`multisig_*`、`parallel_execution_safety`、`state_root`、`witness_*`）——这些**不能作为桥的 Coq 侧依据**。
- **关键现状**：CI 的 `coq-full` 作业跑 `--all`，因此当前**整体为红**（9 个未通过属预期）。这不影响本文档的桥规划，但意味着"把桥结论接入 CI"需等未通过模型重建完成或采用"verified 子集"门禁。

### 1.2 TLA+ 侧（`formal/consensus/model/`）

- `neo_dbft_complete.tla`：**单高度、单视图、honest 票全局可见的 quorum 抽象**（非完整 dBFT）。常量 `n, f, M, Values, None`；`f < n`、`M ≤ n`。`Quorums == {q ⊆ Validators : Cardinality(q)=M}`。不变式：`InvariantAgreement`、`InvariantValidity`、`InvariantPreparedRequiresQuorum`、`InvariantCommitRequiresQuorum`、`InvariantPhaseConsistency`、`InvariantQuorumIntersection`、`InvariantNoEquivocation`、`InvariantFinality`。
- `neo_dbft.tla`：另一份更贴近消息层（`byzCommits`/`committed`）的抽象，含 `SafeQuorum == 2*M - n > f`。
- **TLC 有界证据**（`formal/consensus/model/current-logs/`，本地实测，非伪造）：
  - `tlc_neo_4f1`（n=4,f=1,M=3）：613 generated / 244 distinct，**No error**。
  - `tlc_neo_7f2`（n=7,f=2,M=5）：16073 / 4276，**No error**。
  - `tlc_neo_3f1_M3`（n=3,f=1,M=3）：77 / 42，**No error**。
  - `witness-finalization`：加入 `NoFinalization`，TLC 报 `Invariant NoFinalization is violated`（**非空确 witness**，exit 12），108 distinct。
- CI 门禁 `formal/ci/run_tlc_ci.sh` **fail-closed**：三组 safe 配置全部要求 `Model checking completed. No error` + 非空确 witness（exit 12 + 命名 counterexample），任何缺失配置/工具/校验失败即红。`tla2tools.jar` v1.7.4 经 SHA-256 固定。

### 1.3 CI 流水线（`.github/workflows/formal-verification.yml`）

确认（重读全文）：
- **已移除假 Krilla 作业**。注释原文："There is deliberately no Krilla job: no such tool exists in this repo and the previous placeholder job only echoed success."（§13-14 行）。
- 两道必需门禁：`coq-full`（`check_coq.py --all`，fail-closed）与 `tlc-bounded`（fail-closed TLC + 非空确 witness），最后 `verification-gate` 断言两者 success。
- **注意**：该 workflow 文件当前为 **staged 未提交**的新文件（`git status` 显示 `A`），git 历史中查不到旧 Krilla 作业的删除提交（`git log --all -i --grep=krilla` 为空）。旧作业已被移除/改 fail-closed 这一结论来自 coverage 报告叙事 + 现文件注释，**无法在本仓库当前 git 状态下用提交历史独立复核**——属已充分但非完全机检确证的事实。

---

## 2. 工具评估

### 2.1 Krilla（Rust→Coq 类型导出）

**结论：本环境不可行，且作为"Rust→Coq 桥"的工具大概率不存在。**

证据（本次实测检索）：
- **crates.io**：`GET https://crates.io/api/v1/crates/krilla` 返回的 `krilla` crate 是 *"A high-level crate for creating PDF files"*（LaurenzV/krilla，最新 0.8.2），**与 Rust→Coq 完全无关**。
- **GitHub 仓库检索**（`gh search repos krilla`）：全部命中都是上述 PDF 库及其 fork，没有任何 Rust→Coq 类型导出工具叫 Krilla。
- **GitHub 代码检索**（`gh search code 'krilla coq'`）：无相关结果。
- **本仓库**：`git grep -i krilla` 仅在 `formal-verification.yml` 与 coverage 报告中出现，且两处都说明该工具/作业**不存在**。

诚实的边界声明：
- 我不能绝对排除"某研究组内部存在一个名为 Krilla 的 Rust→Coq 研究原型"。但在**可安装、可维护、可在本环境（Windows/WSL、rustc 1.88+、Coq 8.18）直接使用**的意义上，它**不存在**。即便存在，也属**研究原型**，rustc 1.88+ / Coq 8.18 的兼容性**需实测/待验证**。
- 更宽泛地说：**"Rust 类型→Coq 类型" 这类自动导出工具（如早期 electrolysis / Fiat 系）本就只是"类型搬运"，不是类型检查器，也不产生任何跨语言正确性保证**。它最多消除"手工抄类型"的笔误，无法证明 Rust 程序与 Coq 规格一致。
- 因此：**Krilla 不应出现在任何里程碑的依赖链中**。若未来某天确有一个可用的 Rust→Coq 导出工具，它的角色也**只是可选加速器**，而非桥的正确性来源。

### 2.2 替代方案（低侵入、稳健）

**方案 A —— 手工维护的 Coq↔TLA+ 映射表（主推）**
- 机制：一份**对应登记文件**（如 `formal/bridge/mapping.md` + 机器可读 `formal/bridge/mapping.json`），逐条登记"Coq 项 ↔ TLA+ 项 ↔ Rust 源头"。示例：`CommitMessage.signature`（Coq `list nat`）↔ TLA+ 抽象票 ↔ Rust `Vec<u8>`；`validate_commit`（Coq）↔ `InvariantCommitRequiresQuorum`（TLA+）↔ `has_enough_commits`（Rust）。
- 优点：零外部工具链、版本可控、不引入 rustc 版本漂移风险、与现有 fail-closed CI 完全兼容。
- 局限：映射是**手工声明**，需要评审维护；一致性靠评审而非自动推导（除非叠加方案 B）。

**方案 B —— 共享可序列化互表示（单一事实来源，强化）**
- 机制：对**数值/常量语义**用同一份权威 JSON（`formal/bridge/constants.json`）通过一个生成器（`scripts/bridge-gen.py`）同时产出：
  - 一段 Coq（如 `CoqLib.BridgeConstants`：`f n := (n-1)/3`、`m n := n - f n`、varint 标记表）；
  - 一段 TLA+（如 `BridgeConstants.tla` 的常量/谓词）。
  - 用 CI 步骤校验"两者由同一 JSON 生成，且已提交产物与生成器再跑结果字节一致"（**机检一致点**）。
- 优点：数值/枚举类语义两侧**保证一致**（由生成器而非人手），风险最低、价值密度最高。
- 局限：只覆盖可序列化/数值类语义；结构/推理类对应仍靠方案 A 人工登记。

**推荐组合**：以 **A 为骨架 + B 强化数值/常量语义**。两者都不依赖 Krilla，都不需要改任何源码。

---

## 3. 具体可执行演示（两套语义在"协议规格"层对应）

下面两个演示都以**已通过**的 Coq 模型为 Coq 侧依据，以**已过 TLC** 的 TLA+ 模型为 TLA+ 侧依据，只声明"协议规格层一致"。

### 演示 1：dBFT 的 quorum / commit 守卫

**Coq 侧**（依据 `commit_message_refinement.v`，已通过）：
- `CommitMessage` 四字段；`validate_commit c ⟺ length c.(signature) = 64 ∧ all_bytes_valid`。
- 含义：**单条 commit 消息的局部结构性守卫**（有效签名必须 64 字节）。

**TLA+ 侧**（依据 `neo_dbft_complete.tla`，已通过 bounded TLC）：
- `InvariantCommitRequiresQuorum := ∀ i∈Honest : decided[i]≠None ⇒ Cardinality(CommitSupport(decided[i])) ≥ M`。
- 含义：**委员会层 quorum 守卫**（提交必须达到 M 票）。
- `InvariantQuorumIntersection` / `neo_dbft.tla::SafeQuorum == 2*M - n > f`：**两 quorum 交叠**的安全条件。

**对应关系（不主张精化，只主张规格层一致）**：
- 这两层是**同一 commit 守卫的两个正交面**：局部"消息结构合法"（Coq）+ 全局"票数 ≥ M"（TLA+）。二者组合才构成完整提交条件，与 Rust `context/mod.rs` 的 `CommitMessage::validate()`（64 字节）+ `has_enough_commits()`（≥ M）对应。
- **数值一致性点（方案 B 直接落地）**：Rust `context/mod.rs` 的 `f()=(n-1)/3`、`m()=n-f()`；TLA+ 模型常量 `M`（`tlc_neo_4f1` 用 M=3、`tlc_neo_7f2` 用 M=5、`tlc_neo_3f1_M3` 用 M=3）。三者对 n=4→3、n=7→5、n=3→3 **完全吻合**。可把 `f/m` 公式做成 Coq 引理 `m n = n - (n-1)/3` 与 TLA+ 谓词，由方案 B 生成器保证两边一致，并在 CI 断言三个 (n,M) 采样一致。
- **安全界对应（可证）**：`2*M - n > f`（TLA+ 声明安全界）⟺ 在 `M = n-f, f=(n-1)/3` 下等价于 `n ≥ 3f+1`（Coq 可证明的纯算术事实）。这为"为何这些配置 TLC 全绿"给出 Coq 侧算术依据。

**可验证产物**：
- Coq：`formal/coqlib/dbft_quorum_arithmetic.v`（`f/m` 定义 + `m n = n - (n-1)/3` + `2*m n - n > f n ⟺ n ≥ 3*f n + 1`，全部无 admit，过 `coqc + coqchk`）。
- TLA+：`formal/consensus/model/BridgeConstants.tla`（`F(n) == (n-1)\div 3`、`M(n) == n - F(n)`、`SafeQuorum(n) == 2*M(n) - n > F(n)`）。
- 生成器：`scripts/bridge-gen.py` 从同一 JSON 产出上述两文件；CI 断言再生成一致。

### 演示 2：varint / 序列化 roundtrip（作为对称的轻量佐证）

**Coq 侧**（依据 `varint_encoding_refinement.v`，已通过）：`encoded_len` 分档 1/3/5（u64=9 未证）、`write_var_int`/`read_var_int_prefix`、小值 roundtrip、输出字节 <256。

**Rust 侧**（`neo-io/src/var_int.rs`，只读确认）：常量 `VAR_INT_U16_MARKER=0xFD` 等；`read_var_int_prefix`/`write_var_int`/`encoded_len` 与 Coq 的分档/公式一一对应。

**对应关系**：把 varint 的长度分档与标记做成**共享常量表**（方案 B），Coq 引理 `encoded_len_small/u16/u32` 与 TLA+ 谓词引用**同一生成源**，从而"两侧对同一输入的编码长度判断一致"。边界：Coq 侧明确只证到 u32，u64/9 字节未证——桥必须**如实登记**这一不对称（诚实，不夸大）。

---

## 4. 实施路线图

> 原则：每步都有**可验证产物**；Krilla 不进入依赖链；不主张程序级精化。

### 里程碑 M0 —— 桥基建与事实复核（约 1 周）
- 产出：`formal/bridge/mapping.json`（初始对应表）+ `formal/bridge/mapping.md`（人类可读）；`formal/bridge/constants.json`（f/m、varint 标记等）。
- 验证：mapping 表每项引用**已通过**的 Coq 模型与 **TLC 已绿**的 TLA+ 模型；未通过模型（9 个）明确标"不可用作桥依据"。
- 通过标准：对应表可被团队评审，无"已通过"项引用未通过模型。

### 里程碑 M1 —— 生成器 + dBFT quorum 数值一致点（约 1-2 周）
- 产出：`scripts/bridge-gen.py`；由 `constants.json` 生成 `formal/coqlib/dbft_quorum_arithmetic.v`（含 `m n = n - (n-1)/3` 等引理，过 coqc+coqchk）与 `formal/consensus/model/BridgeConstants.tla`；CI 步骤校验"再生成一致"。
- 验证：`dbft_quorum_arithmetic.v` 通过权威门禁；`BridgeConstants.tla` 经 TLC parse 且三个采样 (n,M)=(4,3),(7,5),(3,3) 与 Rust `context/mod.rs` 的 `f()/m()` 一致。
- 通过标准：方案 B 一致点机检通过。

### 里程碑 M2 —— commit/quorum 守卫的双层对应（约 1-2 周）
- 产出：Coq 侧新增一个**已通过**的引理，把"单条 commit 结构合法（64 字节）"与"委员会 quorum ≥ M"在**规格层**表述为同一完整提交守卫的两面；mapping.json 登记 `CommitMessage::validate ↔ Coq validate_commit ↔ TLA+ CommitSupport≥M`。
- 验证：对应引理过 coqc+coqchk；TLA+ 侧在现有 safe 配置下复跑仍全绿（回归）。
- 通过标准：双层守卫映射有机器可查的 Coq 依据 + TLA+ 不变式，且无 admit。

### 里程碑 M3 —— varint 共享常量表（约 1 周）
- 产出：varint 标记/分档表由方案 B 生成，Coq 与 TLA+ 引用同一源；mapping.json 如实登记 u64/9 字节未证的不对称。
- 通过标准：常量一致点机检；文档如实标注未证项。

### 里程碑 M4 —— CI 接入与审计（约 1 周）
- 产出：把方案 B 一致性与 M1/M2 新增 Coq 模型接入 `check_coq.py` 的 verified 子集或独立 job；证据上传。
- 注意：`coq-full --all` 当前整体为红（9 个未通过）。桥新增模型应挂到"verified 子集"门禁或独立 job，**不要**让桥的绿/红被未通过的遗留模型遮蔽。
- 通过标准：桥相关门禁绿色、可追溯 SHA、`summary.json` 记录。

---

## 5. 风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| **Krilla/外部 Rust→Coq 工具不可用** | 若把它当依赖则整个桥受阻 | 本方案已排除 Krilla；只用手工映射 + 生成器 |
| **rustc 版本漂移**（1.88+/未来） | 任何 Rust 侧导出工具会失配 | 不引入 Rust 侧构建；只读 Rust 语义常量，由手工/生成器镜像 |
| **两边语义抽象不同** | 对应关系站不住或"空真" | 只对应已通过模型；明确规格层而非程序级精化；用方案 B 锁数值点 |
| **Coq 遗留未通过模型（9 个）被误当桥依据** | 桥结论建立在 admit/公理上 | mapping 表强制只引用通过模型；CI 门禁拒绝 admit |
| **CI `coq-full --all` 当前整体为红** | 桥门禁被遮蔽 | 桥单独走 verified 子集 job；不依赖全量绿 |
| **手工映射漂移（A 方案固有）** | 文档与实际脱节 | 结合方案 B 锁关键数值；映射表纳入 PR 评审 |
| **生成器脚本本身出错** | 伪一致 | 生成器极简、单文件；CI 断言提交产物与再生成一致（字节级） |

---

## 6. 诚实的边界与不做的主张

- **不主张程序级精化**：Coq 模型是抽象模型/合同，不是对 Rust 可执行文件的精化证明；`coverage-status` 已明确这一点。桥只主张"协议规格/抽象模型层一致"。
- **不主张哈希/密码学正确性**：Coq 侧 `sha256`、ECDSA 均为抽象参数，不声明抗碰撞/密码学正确。
- **不主张 u64/9 字节 varint**：Coq 侧明确未证。
- **不主张完整 dBFT 覆盖**：TLA+ 是单高度/单视图 quorum 抽象，非完整协议。
- **未通过模型（9 个）不作为桥依据**：必须诚实标注。

---

## 7. 诚实验收标准

**算"桥完成"（可用机器检查/审计的证据，缺一不可）：**
1. 目标语义（如 dBFT quorum/commit 守卫、varint 分档）在 **Coq 侧有已通过引理**（coqc=0 且 coqchk=0 且无 Admitted/Axiom）且 **TLA+ 侧有对应不变式/谓词**（经 TLC 有界验证或可 parse + 数值一致）。
2. **数值一致点由生成器保证**：Coq 与 TLA+ 的共享常量（f/m、varint 标记等）出自同一 JSON，CI 断言再生成字节一致，且采样与 Rust `context/mod.rs`/`var_int.rs` 对齐。
3. 对应关系登记在 `formal/bridge/mapping.json` 且每项引用的两侧模型均为"通过"状态。
4. 桥相关门禁在 CI 绿，证据上传，`summary.json` 记录 SHA。

**只算"规划"（未达桥完成）：**
- 仅有映射表/文档，无 Coq 引理或 TLA+ 对应谓词；
- Coq 引理带 `Admitted`/`Axiom`，或 TLA+ 未过有界验证；
- 仅"手工声明对应"而无数值一致点或不可审计证据；
- 任何依赖 Krilla/外部 Rust→Coq 工具且未实测验证的方案。

**明确结论**：采用"手工 Coq↔TLA+ 映射（方案 A）+ 单一事实来源生成器锁数值常量（方案 B）"。Krilla 不作为依赖；其在本环境不可行。

---

## 附录：本次只读核查的事实锚点（源码路径）

- Coq 权威门禁：`D:\Git\neo-rs\formal\ci\check_coq.py`
- Coq 现状：`D:\Git\neo-rs\reports\formal\coverage-status-2026-09-18.md`
- 桥相关已通过 Coq 模型：`formal/coqlib/{commit_message_refinement,varint_encoding_refinement,consensus_roundtrip_refinement,mempool_validation_refinement,rw_set_minimal}.v`
- TLA+ 模型：`formal/consensus/model/{neo_dbft_complete,neo_dbft}.tla`
- TLC 本地证据：`formal/consensus/model/current-logs/{tlc_neo_4f1,tlc_neo_7f2,tlc_neo_3f1_M3,witness-finalization}.log`
- CI 门禁：`.github/workflows/formal-verification.yml`、`formal/ci/run_tlc_ci.sh`（Krilla 作业已移除/fail-closed）
- Rust 语义源头（只读）：`neo-consensus/src/messages/commit.rs`、`neo-consensus/src/context/mod.rs`（`f()/m()/has_enough_commits`）、`neo-io/src/var_int.rs`
- Krilla 检索结论：crates.io `krilla` = PDF 库（LaurenzV/krilla）；GitHub 无 Rust→Coq 工具叫 Krilla。
