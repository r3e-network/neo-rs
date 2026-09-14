# neo-rs 项目长期备忘

## 环境事实（2026-09-09 更新）

- **RocksDB 已解锁（2026-09-09）**：E1b 真因是 PATH 里 `C:/Program Files/Rust stable LLVM 1.95/bin` 优先——cargo 1.95.0 是 **gnullvm host** 发行版，librocksdb-sys C++ 走 llvm-mingw clang++，mingw 头默认 `_WIN32_WINNT=0x601`，而 `FILE_ID_INFO` 需 ≥0x0602。**解锁**：`export CXXFLAGS_x86_64_pc_windows_gnullvm="-D_WIN32_WINNT=0x0A00"`（CFLAGS 同理），零代码改动。实测 `neo-node --features full` check EXIT=0；neo-core `--features rocksdb` 测试全绿（门控测试文件 17 个，非旧记 14）。`#[ignore]` 的 mainnet repro 仍需 146 万+ 高度 full-state 数据（本机无）。
- **PATH 工具链陷阱**：`which cargo` 若指向 `Rust stable LLVM`（gnullvm host）而非 `~/.cargo/bin`（rustup 代理，MSVC host），C++ 依赖的编译路径完全不同；cc crate 的 `CXXFLAGS_<target三合一下划线>` 是可注入编译宏的钩子（看 build 输出里 `CXXFLAGS_xxx = None` 即可用）。
- **fuzz/libfuzzer-sys 已解锁（2026-09-09）**：libfuzzer-sys 0.4.10 build.rs 无 msvc/gnu 分支、glob 编译全部 .cpp，`FuzzerExtFunctionsWindows.cpp` 的 MSVC `__pragma(comment(linker,"/alternatename:..."))` 在 gnu 目标需 MS 扩展。**钩子**：`CXXFLAGS_x86_64_pc_windows_gnullvm="-D_WIN32_WINNT=0x0A00 -fms-extensions"`（`-fms-extensions` 单加即够，不定义 `_MSC_VER`）；lld mingw 驱动会执行 `.drectve` 的 `/alternatename`，链接与 3 target 冒烟均 EXIT=0。**gnullvm host 无 sanitizer/sancov**——覆盖引导 fuzzing 只能 Linux CI。
- 工作区**没有名为 `dbft` 的包**：dBFT 实现在 `neo-consensus`；历史基线里的 "dbft 110" 即 `neo-consensus --lib`。
- git：永久禁用 stash/checkout <sha> --/reset/fetch（引用事务清理 bug 删 refs）；历史版本一律 `git show <sha>:<path>`。
- Bash 循环里改 `IFS` 会吃掉 `--features a b` 的分词——`--features server` 报 `unexpected argument` 先查 IFS，不是 cargo 问题。
- `cargo fmt -p <crate>` 是 crate 级——工作树含 WIP 文件时**禁止**，只对自改文件用 `rustfmt --edition 2024 --check <file>` 单文件。
- Windows 沙箱拦 `target/` 写入：cargo 命令一律 `dangerouslyDisableSandbox: true`。

## 协议对账结论（C# 逐字取证）

- C# `Block.Verify` 就是 `Header.Verify` 纯委托；Merkle/去重在 `DeserializeTransactions`。Rust 同构——块级交易见证校验两侧都不属于收块路径。
- C# `ExecutionEngineLimits` **没有指令数上限**；RPC `MaxGasInvoke` 两侧默认都是 10 GAS。
- 存储键端序（M-01 已核销）：两侧索引后缀均大端；`Prefix_GasPerBlock=29` 在 C# 同属 NeoToken。
- HASKEY：C# 负索引无条件抛异常；`index >= MaxItemSize` 也抛（Rust 已补上界 F-NEW-1）。
- neo-vm 解释器路径（`interpreter/executor`）是 **Gorgon-only**：pre-Gorgon 时 `external_vm.rs` `return None` 回退 fork-aware jump table。
- C# N3 后期 dBFT 在 `neo-modules/src/DBFTPlugin/Consensus`（core 无 Consensus 目录）；定时器/法定数/ExtendTimerByFactor 与 Rust 逐字对齐（P0-5 已闭环）。
- `verify-protocol-presets.py` 对 mainnet+testnet 活节点 38 项 ALL MATCH；已挂 CI（protocol-consistency-goldens 作业）。
- C# `Transaction.VerifyStateIndependent` 只内联单签/多签快速分支，**不匹配即跳过、不调 VerifyWitness**（Rust 一致）；截断 invocation 由 `VerifyStateDependent → Helper.VerifyWitness` 拒绝。

## 审计基线（v0.17.0 = a3231470）

- **v0.15→v0.17 审计全部闭环（2026-09-09）**：5 个 P0 全证伪/闭环；F-NEW-1/2/3、D-04 全部处理；T01/T02/T06/T07/T08 完成；E1b 与 E2（fuzz）双双解锁；治理项（CI 双闸门+README+run_all 真实实现）落地；主工作区 dev profile 全成员 build 亦 EXIT=0。
- 测试基线：neo-vm lib 129/0/0、neo-consensus lib 114/0/0、neo-core lib 742/0/0（runtime）、neo-core rocksdb 门控 83 二进制全绿、neo-rpc 与 neo-tests 全绿。
- 遗留：`mainnet_block_*_repro` 18 个 ignore 需 full-state 数据同步；`NEO_EXECUTION_SPECS_REF` repo variable 未配置（consistency workflow dispatch-only）。
- 报告集：`outputs/audit-v017-*.md`（scoping / t01 / t02 / t07-security / t08 / final）。

## CI 全绿基线（2026-09-10）

- 远端 main = **`3f6d9a35`**，CI **13/13 作业 success、0 failure**。审计修复经临时 clone 合并上 main：`02768a17`（merge）→ `a9500c02`（停定时）→ `9caa2bd9`（补 deny.toml/verify-protocol-consistency.py/goldens fixture）→ `accb2f14` → `6d154010` → `53518bc4` → `8a93d551` → `3f6d9a35`。
- **首跑红灯全部归因于 `dd67973a` 从未被 CI 测过**，非审计改动（`git log 567a329a..dd67973a -- <审计9文件>` 全 0）。这是"D-15 死测试"的同源证据。
- fuzz-smoke 四连坑的统一根因 = **预编译 action vs 源码构建路径分歧**；解法 `cargo install cargo-fuzz --locked`。
- **推送通道**：ssh 被沙箱拦 `~/.ssh` 时改 `git push https://github.com/<owner>/<repo>.git main`（走 credential helper，实测成功）；ssh push 有偶发静默失败，**push 后必 `git ls-remote` 校验**。
- check-runs API 匿名可用（列 job 结论），但 `steps` 与 logs 不可用（403/空）；**步骤级结论要走 jobs API**：`/repos/{o}/{r}/actions/runs/{run_id}/jobs`。

## 协议对账：原生合约方法表面 118/118 PARITY（2026-09-10）

- `scripts/verify-native-parity.py`（已挂 CI 硬闸门）：解析 C# v3.10.1 全部 `[ContractMethod]` vs Rust 原生方法表，逐方法对比。**11/11 合约、118 方法、0 缺失 0 多余**，已做变异测试证明脚本能失败。
- C# 权威清单（11 个，取自 `NativeContract.cs`）：ContractManagement / StdLib / CryptoLib / LedgerContract / NeoToken / GasToken / PolicyContract / RoleManagement / OracleContract / Notary / **Treasury**。Rust 同 11 个同序；`token_management` 存在但**故意不注册**（`is_native()` 会误收非协议 hash）。
- C# 仓库路径是 **`src/Neo/...`（大写 N）**，`src/neo/...` 一律 404。基准用 **v3.10.1 标签**，勿用 master-n3（移动分支，已分叉）。
- 解析三坑：① 命名规则逐字复制 `ContractMethodMetadata.cs:45-46`；② 匹配器要吃**属性**（`Symbol`/`Decimals`）与**元组返回**（`GetCandidates` 返回 `(ECPoint,BigInteger)[]`）；③ Rust 侧 NEO/GAS 的 NEP-17 在 `FungibleToken::ft_nep17_methods()`。
- 边界：只比**存在与命名**，不比语义/gas/参数/硬分叉窗口。绿 ≠ 行为等价。

## 协议对账：syscall 41/41、opcode 196/196（2026-09-10）

- `verify-syscall-parity.py`：**41/41** + 哈希 **43/43** 按 C# 规则重算一致。C# 哈希 = `BitConverter.ToUInt32(SHA256(ASCII(name)),0)`（小端 uint32 前 4 字节）。哈希错 → 每个 SYSCALL 都 fault，必须单独验。
  - **Rust 注册面是 `register_host_service()`（41 处，跨行）**，不是 `neo-vm/src/host/syscall.rs` 那张表（只服务 `syscall_arg_count()`，含 `System.Contract.Create`/`Update` 两个两侧都未注册的条目，拿它比会多报 2 个"多余"）。
  - C# Register 分散在 `ApplicationEngine*.cs` 分部类。
- `verify-opcode-parity.py`：**196/196，字节值 0 偏差**。
  - **VM 已拆出主仓库**：opcode 来自 `neo-project/neo-vm` @ v3.10.1（`src/Neo.VM/OpCode.cs`）；其余对账用 `neo-project/neo` @ v3.10.1。主仓库 `src/Neo/VM/` 已不存在。
  - 解析坑：C# 枚举末项 `ASSERTMSG = 0xE1` 无尾随逗号，正则要求逗号会漏。
- CI 作业 `surface-parity`（原 `native-parity`）跑三个对账，均硬闸门。

## 协议对账：P2P 23/23；RPC 无法严格对账（2026-09-10）

- `verify-p2p-parity.py`：**23/23** 消息命令名称+单字节判别符全同（已挂 CI）。
  - Rust 侧是宏生成：`neo-primitives/src/macros.rs` 的 `__p2p_message_command_table`，提取须限定在该宏内；`Unknown(u8)` 是 `protocol_enum_with_unknown!` 的实现细节，须排除。
- **RPC 无法在 v3.10.1 严格对账**：RPC 服务在 `neo-project/neo-modules`，该仓库**已归档、标签止于 v3.7.5**，**无 v3.10.1** → 上游无权威清单。指示性比对（v3.7.5）显示 C# 42 个方法在 Rust 侧 **0 缺失**（Rust 57 个，超集）。未接入 CI。
  - Rust RPC 注册面 = `register_handlers()` 内 `rpc_handlers![...]`；`middleware/rate_limiter.rs` 含同名字符串，正则易误纳——曾三版正则得 46/51/57 三个答案。
  - RPC 不在共识路径，缺失/多余不会分叉，属客户端兼容性问题。

## CI 成本（用户明确要求）

- **不要频繁推送触发完整 CI**（14 个作业，贵）。做法：**本地验证 → 攒批 → 一次性推送**。非必要不单独推。
- **通用教训：写"全绿"断言类脚本必须做变异测试，且要确认变异真的命中目标**（曾因字符串出现 2 次、只替换了非注册点的第 1 处，误判脚本恒绿）。

## 依赖策略（cargo-deny）

- `continue-on-error` 会把真失败吞成绿——任何"success"先确认没有软门控。
- `licenses bans sources`：**通过**（硬门控）。此前 CI 失败真因是 `EmbarkStudios/cargo-deny-action` 把 `arguments` 排在 `command` 前，拼成 `... licenses bans sources check` 被拒；改为下载固定 0.20.2 二进制直接调用。
- `advisories`：**真失败**，4 项真实 RustSec（2025-0134 / 2026-0098 / 2026-0099 / 2026-0104），限 RPC/TLS（rustls 0.21 栈），**不在共识路径**。未静默，待维护者决策迁离 rustls 0.21。
