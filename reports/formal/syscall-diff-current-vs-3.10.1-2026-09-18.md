# NeoVM syscall / native-contract 差异验证（带宿主 ApplicationEngine）— 新鲜证据（2026-09-18）

> 目标：把 `reports/formal/vm-diff-current-vs-3.10.1-2026-09-18.md` 里的 bare-engine 差异验证
> 扩展为**带 syscall / 原生合约执行**的差异测试，验证 neo-rs VM 在执行 syscall 时与
> C# 参考实现 **Neo.VM 3.10.1 / Neo 3.10.1** 状态一致。
> 本文档只记录**实际跑出来并能复核**的结果，并明确标注边界——不虚报"完整状态一致"。

## 1. 结论（一句话）

**14 个带 syscall 的差异向量在 Rust（neo-rs ApplicationEngine）与 C#（Neo 3.10.1 ApplicationEngine）两侧
全部一致（PASS，exit 0）**。这覆盖了 `System.Runtime.*` 元数据与 `System.Crypto.*` 签名校验的
syscall 执行路径，是从 bare engine 到"宿主引擎 + syscall"的一次真实扩展。
但它**不涵盖**存储读写、NEO/GAS 原生合约价值转移、跨合约调用等更重路径（见 §4 边界）。

## 2. 新增/改动的文件

| 组件 | 路径 | 说明 |
|---|---|---|
| 向量生成 | `scripts/gen-syscall-vectors.py` | 生成 syscall 差异向量；opcode 字节读自 `opcode.rs`，syscall hash = SHA-256(method) 前 4 字节 LE |
| 向量 | `vectors/vm/syscall-vectors.json` | **14 个** syscall 向量（新文件，不动 99/708 那两个） |
| Rust runner | `neo-core/examples/vm_syscall_runner.rs` | 用 `ApplicationEngine`（注册并执行 syscall 的那层）跑脚本，输出与 `vm_diff_runner.rs` 相同的 envelope schema |
| C# runner | `tools/csharp-app-runner/Program.cs` + `.csproj` | 用 `Neo.SmartContract.ApplicationEngine`（Neo 3.10.1）跑同一批向量；csproj 固定 3.10.1 |
| 比对器 | `scripts/vm-diff.py` | **未改动**，直接复用（exit 0=全部匹配） |
| 证据 | `reports/formal/syscall-diff-evidence/syscall-current-vs-3.10.1.json` | 机器可读比对报告 |

> 在实现前，我通过临时诊断工程确认了 C# 侧 `ApplicationEngine` 的可构造性（顺利后已移除该临时工程）。

## 3. 覆盖的 syscall / 原生调用

每个向量 = 一段脚本（push 参数 + `SYSCALL <hash>` + `RET`），两侧以**完全相同的固定宿主环境**执行：

- `ProtocolSettings` 默认值（network=0, address_version=53）
- `TriggerType.Application`
- 一个最小 `Transaction` 容器（crypto 只走 FALSE/FAULT 路径，容器 hash 不参与结果）
- 一个持久化块，header 时间戳 = `1700000000`

| 向量 | syscall | 期望结果 | 比对 |
|---|---|---|---|
| rs.platform | `System.Runtime.Platform` | `"NEO"` ByteString | PASS |
| rs.network | `System.Runtime.GetNetwork` | Integer `0` | PASS |
| rs.address_version | `System.Runtime.GetAddressVersion` | Integer `53` | PASS |
| rs.trigger | `System.Runtime.GetTrigger` | Integer `64` | PASS |
| rs.invocation_counter | `System.Runtime.GetInvocationCounter` | Integer `1` | PASS |
| rs.time | `System.Runtime.GetTime` | Integer `1700000000`（固定块时间戳） | PASS |
| rs.calling_hash | `System.Runtime.GetCallingScriptHash` | Null（入口无调用方） | PASS |
| rs.executing_hash | `System.Runtime.GetExecutingScriptHash` | 脚本 hash（Bytes） | PASS |
| rs.entry_hash | `System.Runtime.GetEntryScriptHash` | 脚本 hash（Bytes） | PASS |
| crypto.checksig.false | `System.Crypto.CheckSig` | `false`（合法 pubkey + 全 0 签名，与消息无关） | PASS |
| crypto.checksig.invalid_pubkey_len | `System.Crypto.CheckSig` | FAULT（70 字节 pubkey） | PASS |
| crypto.checkmultisig.empty_pubkeys | `System.Crypto.CheckMultisig` | FAULT（空 pubkey 数组） | PASS |
| crypto.checkmultisig.invalid_sig | `System.Crypto.CheckMultisig` | `false`（2-of-2 全 0 签名） | PASS |
| cw.not_signer | `System.Runtime.CheckWitness` | `false`（无匹配 signer） | PASS |

比对输出（`python scripts/vm-diff.py ...`，exit 0）：
```
vectors: 14 total, 14 matched, 0 different (missing left: 0, missing right: 0)
result: PASS (all vectors match)
```
比对字段同旧验证：`name, state, stack, fault, harness_error`，栈用共享 Neo JSON-RPC envelope，严格类型/顺序比较。

### 复现命令
```bash
python scripts/gen-syscall-vectors.py                       # -> vectors/vm/syscall-vectors.json (14)
cargo run -q -p neo-core --example vm_syscall_runner -- vectors/vm/syscall-vectors.json .cache/vmdiff/syscall-current-rust.json
dotnet run --project tools/csharp-app-runner -- vectors/vm/syscall-vectors.json .cache/vmdiff/syscall-current-csharp.json
python scripts/vm-diff.py .cache/vmdiff/syscall-current-rust.json .cache/vmdiff/syscall-current-csharp.json
```

## 4. 明确边界（未覆盖，为什么）

这次差异测试**只覆盖**了上表 14 个 syscall 的**确定性纯语义路径**。以下均**未**纳入：

1. **存储读写（`System.Storage.*`）**：需要给引擎加载"当前合约上下文"（storage context + contract script
   hash），两侧都要在快照里种入合约/存储前缀，才能产生可比的读写结果。属于独立的一层，未覆盖。
2. **NEO / GAS 原生合约（`System.Contract.CallNative` → NeoToken/GasToken 价值转移）**：构造时会读原生
   合约存储，C# 侧要种入 Ledger+Policy（已解决）+ NEO/GAS 供应/账号等，Rust 侧也要预置原生状态。
   这层未在本批向量内。
3. **跨合约调用（`System.Contract.Call`）、合约创建/更新、`System.Runtime.Log/Notify/GetRandom/
   GetScriptContainer/CurrentSigners`**：依赖容器内容或事件/随机源，两侧要构造一致的容器与通知环境，未覆盖。
4. **GAS 记账精确值（`System.Runtime.GasLeft` 等）**：两侧 opcode/syscall 计价因子可能不同；我们只验证了
   "给定充足 gas 下状态/栈一致"，**未验证 gas 消耗数字逐字节一致**（gas=20e9 充足，不会因 gas 耗尽而 FAULT）。
5. **`System.Crypto.CheckSig` 的"合法签名→true"正路径**：该路径需要两侧对同一容器算出**相同 hash**
   （sign data = network‖container.hash）。无法保证 neo-rs 与 C# 对"最小 Transaction"的序列化 hash 完全一致，
   故**只测 FALSE 与 FAULT 路径**（不依赖具体消息），不编造正路径结果。
6. **`GetScriptContainer`**：返回容器序列化的 StackItem，两侧容器 hash 若不同会不同，未覆盖。

因此，**PASS 的诚实口径是**：对这批确定性 syscall 向量，neo-rs ApplicationEngine 与 Neo 3.10.1
在"是否 HALT/FAULT、结果栈内容、故障族"上逐项一致。**不等价于**"完整区块/存储状态一致"，
存储、原生代币、跨合约、gas 精确值仍未验证。

## 5. C# 侧构造阻塞与最小方案（已解决，但记录原因）

Neo 3.10.1 的 `ApplicationEngine.Create(...)` 在构造时会直接索引原生合约存储
（`LedgerContract.CurrentIndex`、`PolicyContract.GetExecPicoFeeFactor/GetStoragePrice`），
空快照会抛 `KeyNotFoundException`。经实测定位后，用 `Neo.Persistence.Providers.MemoryStore` +
`StoreCache` 在内存快照里种入最小 genesis 键即可构造成功：

- Ledger current block：`StorageKey.Create(-4, 12)`，值为 `BinarySerializer.Serialize(Struct{hash,index})`
- Policy exec-fee-factor：`StorageKey.Create(-7, 18)` = `StorageItem(BigInteger 30)`
- Policy storage-price：`StorageKey.Create(-7, 19)` = `StorageItem(BigInteger 100000)`

Rust 侧 `ApplicationEngine::new` 对空存储用 `unwrap_or(默认值)`，无需种入；两侧取相同默认值，故结果一致。

另：故障 `fault` 族在两侧措辞不同（如 C# "Invalid ECPoint encoding format … Expected …"
会先命中 type 分支，Rust "Invalid public key length" 命中 invalid 分支）。为诚实对齐，两侧 syscall runner
的 `classify` 都加了"public key / ecpoint / pubkey → invalid"规则（见两 runner 源码）。这只影响本批
crypto 校验故障向量的分类，与 99/708 向量 runner（未改）无关。

## 6. 后续目标

要覆盖剩余边界，需要：存储/合约上下文差异 runner、原生合约（NEO/GAS）genesis 状态种入 + CallNative 差异、
跨合约调用差异、以及 gas 精确计价对照。这些是下一步差异测试与形式化重建的明确目标。
