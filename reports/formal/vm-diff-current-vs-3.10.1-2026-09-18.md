# NeoVM 与 Neo.VM 3.10.1 差异验证 — 新鲜证据（2026-09-18）

> 目标（用户）：验证 neo-rs 完整实现与 Neo N3 v3.10.1/2 **状态一致、100% 兼容、不产生状态不一致**。
> 本文档给出**可用当前代码复核**的机器验证证据，并明确标注其**边界**——避免把"VM 执行语义一致"误报为"完整区块状态一致"。

## 1. 工具链（已存在，本次复核）

| 组件 | 路径 | 版本/基线 |
|---|---|---|
| Rust runner | `neo-vm/examples/vm_diff_runner.rs` | `rust-neo-vm`（bare engine，无宿主/syscall/存储） |
| C# runner | `tools/csharp-vm-runner/Program.cs` | **Neo.VM 3.10.1**（csproj 固定，勿浮动）+ Neo 3.10.1 |
| 向量生成 | `scripts/gen-vm-diff-vectors.py` | 从 `neo-vm/src/vm/opcode.rs` 读取字节，防字节漂移 |
| 链上向量 | `scripts/gen-chain-vm-vectors.py` | 从 MainNet 真实区块脚本提取 |
| 严格比对器 | `scripts/vm-diff.py` | exit 0=全部匹配，1=差异，2=输入错 |

C# runner 的 csproj 明确注释 "Pinned to the audit baseline. Do NOT float this version."，即差异基线锁定 3.10.1。

## 2. 本次用当前代码重跑的差异验证（非过期缓存）

之前的 `.cache/vmdiff/*.json` 均为 **2026-09-14** 生成，早于本会话对 VM/语义的改动，不能作为"当前代码"证据。故本次**用当前代码重新生成并重跑**：

| 向量集 | 数量 | 内容 | Rust 结果 | C# 3.10.1 结果 | 比对 |
|---|---|---|---|---|---|
| `vectors/vm/diff-vectors.json` | 99 | 合成纯 VM：算术/比较/位/栈/流/类型/字节/转换/元数据，含故障分支 | `.cache/vmdiff/current-rust.json` | `.cache/vmdiff/current-csharp.json` | **PASS, 0 different** |
| `vectors/vm/chain-vectors.json` | 708 | 27 个 MainNet 真实区块中交易的 invocation/verification 脚本（覆盖 Aspidochelone..Gorgon 各 hardfork 高度 + 深 Merkle 块） | `.cache/vmdiff/current-rust-chain.json` | `.cache/vmdiff/current-csharp-chain.json` | **PASS, 0 different** |

比对输出（`python scripts/vm-diff.py ...`，exit 0）：
```
99 vectors, 99 matched, 0 different  → PASS
708 vectors, 708 matched, 0 different → PASS
```
比对字段：`name, state, stack, fault, harness_error`，栈用两侧共享的 Neo JSON-RPC stack envelope，严格类型/顺序比较（不把 `True` 当 `1`）。

## 3. 这意味着什么（诚实口径）

**已证明**：对这两组向量，Rust VM 与 Neo.VM 3.10.1 在**执行终止状态（HALT/FAULT）、结果栈、故障分类**上逐项一致。这覆盖了 opcode 边界、算术溢出/截断、栈深/下溢、比较、位运算、数组/结构/Map、字节操作、类型转换、控制流、跳转——是"状态一致"在 **VM 指令执行语义**层面的机器证据。

**未证明 / 边界（必须声明）**：
- runner 是 **bare engine**：无宿主、无 syscall 注册、无存储。因此 **Syscall 本身、原生合约（NEO/GAS/Crypto/Contract/…）、存储读写、跨合约调用、GAS 记账、硬分叉内的应用引擎状态转换**均未在此差异验证内。
- 这**不等价于"完整区块状态一致"**。区块状态（state root、MPT、原生合约余额、存储键值）取决于 ApplicationEngine + native contract + storage 层，本差异验证只覆盖了 VM 内核。
- 链上向量中的 syscall 脚本在两侧都预期 FAULT（"Syscall … not registered"）——这本身验证了两侧对"未注册 syscall 报错"一致，但**没有验证已注册 syscall 的执行结果一致**。

## 4. 复现命令

```bash
# 生成/确认向量（当前 opcode.rs 派生）
python scripts/gen-vm-diff-vectors.py            # -> vectors/vm/diff-vectors.json (99)
# Rust 侧（当前代码）
cargo build -p neo-vm --example vm_diff_runner
cargo run -q -p neo-vm --example vm_diff_runner -- vectors/vm/diff-vectors.json .cache/vmdiff/current-rust.json
cargo run -q -p neo-vm --example vm_diff_runner -- vectors/vm/chain-vectors.json .cache/vmdiff/current-rust-chain.json
# C# 侧（Neo.VM 3.10.1）
dotnet run --project tools/csharp-vm-runner -- vectors/vm/diff-vectors.json .cache/vmdiff/current-csharp.json
dotnet run --project tools/csharp-vm-runner -- vectors/vm/chain-vectors.json .cache/vmdiff/current-csharp-chain.json
# 比对
python scripts/vm-diff.py .cache/vmdiff/current-rust.json .cache/vmdiff/current-csharp.json
python scripts/vm-diff.py .cache/vmdiff/current-rust-chain.json .cache/vmdiff/current-csharp-chain.json
```

## 5. 结论与缺口

- **当前代码在 99+708 个 VM 差异向量上与 Neo.VM 3.10.1 全部一致（PASS）**。这是可复核的、与用户基线版本（v3.10.1）匹配的 VM 语义证据。
- 要达成用户要求的"完整状态一致、不产生状态不一致"，**尚缺**：syscall/native-contract 差异测试、ApplicationEngine 语义差异测试、存储/MPT 状态差异测试、以及区块/交易序列化 roundtrip（对应 `block_transaction_roundtrip`/`serialization` 等尚待重建的 Coq 模型）。这些是后续差异测试与形式化重建的明确目标。