# Prusti 与 Verus 工具链评估 — 诚实报告（2026-09-19）

> 范围：对 Prusti 与 Verus 做 **实测** 工具链评估，给出选型建议与可运行试点路径。
> 方法：所有结论基于本机 `D:\Git\neo-rs`（Windows 10 x64）**实际执行验证器** 得到的输出；凡未实测的能力一律标注「未实测/待验证」。
> 触发目标（VERIFICATION-PLAN / FINAL-REPORT 中）：`Evaluate Prusti upgrade options or Verus Linux deployment`。

---

## 0. 结论速览（TL;DR）

- **两台工具在本机 Windows 上当前都实际可运行并通过了真实函数验证**（与早前 `FINAL-REPORT.md`「impossible here」、`.tools/VERUS-INSTALLATION-SUMMARY.md`「DLL 失败」的结论矛盾，那两份是**过时/错误**的）。
- **Prusti 0.2.2（2024-03-26）**：`prusti-rustc.exe` 在本机对 `formal/prusti/pilot_standalone.rs` 输出
  `Verification of 14 items... Successful verification of 14 items`。已拥有的 `pilot_*.rs` **不是**「无法运行的未验证草稿」——它们在现行安装上**已被真实验证通过**。
- **Verus 0.2026.09.13**：`verus.exe` 对本机输出的 `formal/prusti/pilot_verus.rs`（本次新增）输出
  `7 verified, 0 errors`。无需 Java、无需老 nightly，是**新发行、活跃维护**的工具。
- **选型建议**：对于目标「约 200 个纯函数属性验证」，**优先 Verus**（Windows 即可运行、无需 Java、Z3 后端、数值/编码/序列化友好、版本与 rustc 1.98 匹配，已装机）。Prusti 仅在有匹配当前 MSRV 的新构建时才值得用于 crate 级；现装 Prusti 0.2.2 只能验证独立文件，**不能**接入仓库工具链（MSRV 1.88 / stable 1.95）。

---

## 1. 现状（读入的既有资产）

| 位置 | 内容 | 状态 |
|---|---|---|
| `formal/prusti/pilot_specs.rs` | 含 `extern crate prusti_contracts`、`#[pure]`/`#[requires]`/`#[ensures]` 例（UInt160/256 长度、opcode 价格、charge_gas、syscall 地板） | 头部自称「UNVERIFIED until a compatible Prusti runs」 |
| `formal/prusti/pilot_standalone.rs` | 无 `extern crate`、14 个 `#[pure]` 项（含地址/UInt 校验、C# 参考价格、gas 双扣、预算上限） | 头部自称「UNVERIFIED」 |
| `reports/formal/coverage-status-2026-09-18.md` | Coq 34 模型 25 过 / 9 伪；TLA+ 正常；**未实测 Prusti/Verus** | 权威门禁为 Coq，本文与之无冲突 |
| `reports/formal/FINAL-REPORT.md` / `VERIFICATION-PLAN.md` | 声称 Prusti pilot「14 项已 verified」**同时**声称「impossible here / toolchain unavailable」 | **自相矛盾、已过时** |
| `scripts/formal-verification/run_prusti.sh` | Prusti 复现脚本（设 JRE 21、跑 `prusti-rustc --edition 2021`） | 存在且可用 |
| 仓库工作区 | MSRV 1.88、当前 stable 1.95（`rustc --version` = 1.95.0）；Cargo 无 viper/java 依赖（搜索 `--include=Cargo.toml` 无命中，除 `.tools/verus-source/` 内部） | — |

**工具链实测**（`rustup toolchain list`）：
- `stable-x86_64-pc-windows-msvc`（active，默认）= rustc 1.95.0
- `1.88.0`, `1.86.0`, `1.75.0`（均 msvc/gnu）
- `1.98.1-x86_64-pc-windows-msvc`（**Verus 源树指定，已装，编译 Verus 目标用**）
- `nightly-2023-09-15`（**Prusti 0.2.2 指定**，已装）
- `nightly-2025-12-07`, `nightly-2026-08-27/28`

**Java / Viper / Z3 实测**：
- `java` = OpenJDK 17（AdoptOpenJDK，`JAVA_HOME` 已设）；winget 另有 **Temurin JRE 21.0.12**（`Eclipse TAdoptium.Temurin.21.JRE`）→ Prusti 的 Viper 后端可用（run_prusti.sh 也显式指向 JRE 21）。
- `.tools/viper_tools/`（backends/boogie/resources/z3）存在 → Prusti→Viper→Boogie 链路完整。
- `.tools/z3.exe` 4.16.0 → Verus 的 SMT 后端可用。

---

## 2. 环境实测（谁装了什么、能否运行）

> 判定方式：**直接执行验证器对真实函数跑出「N verified / 0 errors」**，非仅 `which`。

### 2.1 Verus — **已安装且实际可运行** ✅
- 二进制：`~/.cargo/bin/verus.exe`；`.tools/verus.exe`、`rust_verify.exe`、`cargo-verus.exe`、`z3.exe`、全套 `.rlib/.dll/.vir`。
- 版本：`./verus.exe --version` → `0.2026.09.13.671956e`（release, windows_x86_64, toolchain 1.98.1-msvc）。**发行日期 2026-09-13，极新。**
- 实测验证：`./verus.exe formal/prusti/pilot_verus.rs` → **`7 verified, 0 errors`**（本次新增，7 项均为真实 neo-rs 纯函数属性）。
- 与 `VERUS-INSTALLATION-SUMMARY.md`（2026-09-17）记录的**「空 DLL 依赖错误、无法独立执行」矛盾**——该摘要已过时；现 Binary 在 `.tools/` 目录内运行正常（依赖 DLL/rlib 与 z3 均同目录解析）。
- 需要的工具链 `1.98.1` 已装且是 Verus 源树 `rust-toolchain.toml` 指定版本，**与仓库 MSRV 1.88/stable 1.95 完全独立**，不污染仓库默认工具链。

### 2.2 Prusti — **已安装（旧版本）且对独立文件实际可运行** ✅（受限于老工具链）
- 二进制：`.tools/prusti-rustc.exe`、`prusti-driver.exe`、`cargo-prusti.exe`（Mar 26 2024）。
- 版本：`./prusti-rustc.exe --version` → `Prusti 0.2.2, commit 0d4a8d4 2024-03-26`，构建于 `rustc 1.74.0-nightly (2023-09-14)`。`.tools/rust-toolchain` = `nightly-2023-09-15`（已装）。
- 实测验证：`./prusti-rustc.exe --edition=2021 formal/prusti/pilot_standalone.rs` → **`Verification of 14 items... Successful verification of 14 items`**；单函数 `charge_gas` → `2 items` 通过。
- 后端：Viper（`viper_tools/`）经 Java 17（JAVA_HOME 已设；脚本另指 JRE 21）→ Boogie → Z3，链路实际走通。
- **注意**：Prusti 0.2.2 针对 **2023-09-14 的 nightly** 构建，远老于仓库 MSRV 1.88 / stable 1.95，**不能**直接编译/验证仓库 crate 内代码（crate 级集成需匹配当前 rustc 的 Prusti 新构建，**未实测可用，标注待验证**）。它能做的是对**独立放行 `prusti_contracts` 注解的 `.rs` 文件**做验证。
- `cargo-prusti.exe --version` 拒绝 `--version`（应走 `cargo prusti`），未在 PATH，需从 `.tools/` 显式调用。

### 2.3 装机结论表
| 工具 | 未装/已装 | 能否运行 | 对仓库@MSRV 1.88/1.95 | 备注 |
|---|---|---|---|---|
| Verus 0.2026.09.13 | 已装 | ✅ 7 verified | 独立（1.98.1） | 新、活跃、无需 Java |
| Prusti 0.2.2 (2024-03-26) | 已装（旧） | ✅ 14 items 独立文件 | ❌ 不兼容，仅独立文件 | 需 Java/Viper；crate 级待升级后验证 |
| Java JRE | 17 active，21 可用 | ✅ | — | JRE 21 via winget，run_prusti.sh 用它 |
| Viper | `viper_tools/` | ✅ | — | Prusti 后端，实际走通 |
| Z3 | `.tools/z3.exe` 4.16.0 | ✅ | — | Verus 后端 |

---

## 3. 两工具对比

| 维度 | Prusti (0.2.2) | Verus (0.2026.09.13) |
|---|---|---|
| 后端 | Viper（Java）→ Boogie → Z3 | SMT **Z3**（无 Java） |
| 规范语法 | 注解属性 `#[pure] #[requires] #[ensures]`，贴近原生 Rust | 显式 `spec fn` / `proof fn` / `exec fn` + `requires/ensures` 块 |
| 环境要求 | 需 JRE 17/21 + 老 nightly；安装与 rustc 版本强绑定 | 二进制即跑：`verus.exe file.rs`；需匹配工具链（已装 1.98.1） |
| 与仓库 MSRV 1.88 / stable 1.95 兼容 | **不兼容**（构建于 2023-09 nightly） | **兼容/独立**（用独立 1.98.1 工具链，不侵入默认） |
| 目标场景适用性 | UInt 长度、opcode 非负、gas 非负/兜底、syscall 地板、编码/结构（有 Viper 代数支持） | 同样覆盖 + **数值不变式更强**（spec 用 `int` 线性算术，规避 i64 下溢困扰）、vstd 提供 `Seq/Set/Map` 利于序列化/哈希结构 |
| 哈希/加密原语 | 不主张抗碰撞，宜作不透明契约 | 同；可作 spec/ghost 抽象，不主张抗碰撞 |
| 维护活跃度 | 安装是 2024-03；crate 生态更新版本未在本机验证 | **极活跃**：2026-09-13 发行近在眼前 |
| Windows 可用性 | ✅（需老 nightly + JRE） | ✅ **实测 Windows 直跑**（无需 WSL；Verus Linux/WSL 亦官方支持） |
| 机器整字 | spec 内 i64 减法在实现中需证明不溢出/转 int | spec 用 `int`（数学整），实现 `ensures result as int == ...` 归约回 i64 自动证 |

**最小可运行对比示例（同一属性：gas 扣减永不为负——`neo-rs` `charge_gas`）**：

*Prusti 注解写法（已在 `pilot_standalone.rs`，实测 2 项通过）：*
```rust
use prusti_contracts::*;
#[pure]
#[requires(available >= 0)]
#[requires(cost >= 0)]
#[ensures(result >= 0)]
pub fn charge_gas(available: i64, cost: i64) -> i64 {
    if cost > available { 0 } else { available - cost }
}
```

*Verus spec/proof 写法（已在 `pilot_verus.rs`，实测含此项，7 项通过）：*
```rust
use vstd::prelude::*;
verus! {
spec fn charge_gas_spec(available: int, cost: int) -> int { if cost > available { 0 } else { available - cost } }
exec fn charge_gas(available: i64, cost: i64) -> (result: i64)
    requires available >= 0, cost >= 0
    ensures  result as int == charge_gas_spec(available as int, cost as int), result >= 0
{ if cost > available { 0 } else { available - cost } }
}
```

---

## 4. 诚实验收与已留档证据

| 验收项 | 结果 | 证据（本机实测） |
|---|---|---|
| Prusti 对至少一个真实目标函数验证通过 | ✅ | `pilot_standalone.rs` 14 项 + `charge_gas` 2 项：`Successful verification of 14 items`（exit 0） |
| Verus 对至少一个真实目标函数验证通过 | ✅ | `pilot_verus.rs` 7 项：`7 verified, 0 errors`（exit 0） |
| 覆盖的目标纯函数属性 | 长度(20/32)、opcode 非负 + C# 参考价、gas 非负/兜底/上限、double-charge、syscall 地板、UInt160<UInt256 | 触发「编码 / 哈希 / 序列化 / 数值不变式」目标中的**数值不变式 + 长度/结构**项 |
| 未实测项（**诚实标注**） | 仓库 crate 级（`neo-*`）整链接验证；Prusti 匹配 1.95 的新构建可用性；Verus 对**完整**序列化 roundtrip 的大 spec；真实 SHA-256 建模（非不透明契约） | — |

---

## 4.1 后续扩展与 Prusti 0.2.2 已知限制（2026-09-19 追加）

在盘点基础上继续「补齐 spec」期间，把批量验证规模从 14 项扩展到 **34 项 Prusti spec**：
`pilot_standalone.rs`（14）+ `pilot_batch.rs`（8）+ `pilot_batch2.rs`（12，含 StackItemType 标签归一化、PUSH0/PUSHDATA1/APPEND 与存储/运行时/CheckWitness 等价格常量）。`pilot_batch2.rs` 经 `prusti-rustc --edition 2021` 实测输出 `Successful verification of 12 items`（exit 0）。

过程中用最小探针（`bisect_tags*`、`bisect_clamp`）定位出 **Prusti 0.2.2 纯函数编码的两个硬限制**，均为真实 ICE（`prusti-viper/src/encoder/mir/pure/pure_functions/interface.rs:255 expect_failed`），成因与规避：

| 限制 | 现象 | 规避（已在 `pilot_batch2.rs` 落地） |
|---|---|---|
| `1 << N` / `Shl` 不支持 | 编译 panic「不支持 operation 'Shl'」 | 直接写**字面量**（`1<<3→8`、`1<<15→32768`）或用乘法 |
| 纯函数内 `match` | `interface.rs:255` expect_failed panic | 改写为 **等价的 `if/else` 链**（语义不变，未知标签 passthrough 保留） |

对照实验：零参函数、参数 + `if/else` 分支均正常验证；带 `match` 的**参数化**纯函数即触发 ICE。这些限制只影响「注解式书写风格」，不影响可表达的属性集合——同一属性用 Verus 的 `spec fn`/`if` 表达完全无障碍。两条探针文件为诊断用途，已删除，仅留报告证据。

---

## 5. 选型建议

**推荐：Verus 作为「纯函数 + 数值/编码/序列化不变式」主力层**，理由：
1. 本机 **Windows 直跑、实测通过**，无需 Java/Viper，无老 nightly 约束；版本新、维护活跃（2026-09-13）。
2. spec 用 `int` 线性算术 + `ensures result as int == ...` 已在 `charge_gas`/`charge_gas_twice`/`syscall_gas_floor` 上自动化通过，**对数值不变式（gas、价格表、楼层）最省心**。
3. vstd 的 `Seq/Set/Map` 与 `&[u8]` 长度推理利于种类根/地址校验与 wire 长度/roundtrip 的结构属性；哈希原语作不透明 spec（不主张抗碰撞，与既有 Coq 做法一致）。

**Prusti 的定位（可选，二线）**：
- 仅在获得**匹配当前 MSRV 1.88 / stable 1.95 的 Prusti 新构建**后，才值得做 crate 级内联验证（需 Java/Viper，安装步骤多）。现装 0.2.2 只能对独立文件，**crate 级集成未验证**。
- 结论：**不建议**把「升级 Prusti 到仓库工具链」作为优先投入，除非团队特别偏好注解式（贴原生 Rust）语法且能可靠拿到匹配 nightly 的构建。

**两者策略（推荐）**：
- **Verus 做纯函数/数值/编码/序列化层**（目标「约 200 个纯函数属性」的主载体，Windows 即可跑）；
- **Coq/差分/模型检查继续做协议层**（dBFT、共识消息、block/wire roundtrip——Coq 已 25 模型通过，不需要 Verus 全量替代）；
- Prusti pilot 作为**已完成的独立样例保留**（14 项已实证），但**不**作为仓库 CI 强制（工具链不兼容，脚本 `run_prusti.sh` 保留供复现）。

---

## 6. 分阶段试点路线与验收标准

**Phase 1 — 复现与留档（本周可完成，本报告已部分达成）**
- ✅ 用现装 Prusti 0.2.2 复跑 `pilot_standalone.rs`（14 项通过）；新增 `pilot_verus.rs`（7 项通过）。
- ✅ 记录 SHA/版本输出、exit code、工具链（昼夜留档）。
- 验收：两份运行日志 + 本报告作为证据；不再说「无法运行」。

**Phase 2 — Verus 扩展到真实纯函数（验收＝「评估完成」）**
- 选至少 3 个真实 `neo-*` 纯函数（如 `neo-vm` 的 opcode 价格表索引函数、`neo-primitives` 的 UInt160/256/大数以大小写、`neo-io` 的 varint/wire 读写**若为纯**），各自写成 `exec fn` + `spec fn`，`verus.exe` 跑出 `N verified, 0 errors`。
- **验收标准（「评估完成」的硬指标）**：至少一个**真实目标函数**被工具实际验证通过并留档（含 `verus.exe` 版本、文件、输出、exit），本报告 Phase 1 已满足该硬指标的最低档。
- 若要求「crate 级」，另需 `cargo-verus.exe` 整工作区验证（仅对无副作用纯子集），**此为延伸项，标注待验证**。

**Phase 3 — CI 接入（可选）**
- 新增 `scripts/formal-verification/run_verus.sh`：`cd .tools && ./verus.exe ../formal/prusti/pilot_verus.rs`，exit 0 即绿。不得改动现有源码/`pilot_*`。

---

## 7. 复现命令（原样可跑）

```bash
# Prusti：验证既有 pilot（14 项）
cd D:/Git/neo-rs/.tools
./prusti-rustc.exe --edition=2021 D:/Git/neo-rs/formal/prusti/pilot_standalone.rs   # -> Successful verification of 14 items

# Verus：验证新增 pilot（7 项）
cd D:/Git/neo-rs/.tools
./verus.exe D:/Git/neo-rs/formal/prusti/pilot_verus.rs   # -> 7 verified, 0 errors

# 复现脚本（Prusti）
D:/Git/neo-rs/scripts/formal-verification/run_prusti.sh
```
（均无需网络、无需新装；工具链均已装：nightly-2023-09-15 与 1.98.1。）

---

## 8. 诚实免责

- 本文「已验证」仅指**在指定版本的验证器上，对给定 `.rs` 文件跑出的实际输出**；**不是**对整棵 `neo-rs` 代码库或者对 Rust 可执行语义的精化证明（与 Coq 免责口径一致）。
- 仓库 crate 级验证、Prusti 匹配 MSRV 的新构建、Verus 完整序列化 roundtrip、真实 SHA-256 建模均**未实测，标为待验证**。
- 早前文档（`FINAL-REPORT.md` 的「impossible here」、`.tools/VERUS-INSTALLATION-SUMMARY.md` 的「DLL 失败」）与本文实测冲突，**按本文实测为准**。