# Wire 序列化差异验证 — block / transaction / header roundtrip vs Neo 3.10.1（2026-09-18）

> 目标：验证 neo-rs 的 wire 层序列化（`Block` / `Transaction` / `Header` 的 `Serialize↔Deserialize`，
> 以及底层 varint / varbytes 原语）与 C# 参考实现 **Neo 3.10.1** 双向一致、字节级兼容。
> 任务范围是**序列化 wire roundtrip**，不是区块状态（MPT/原生合约/ApplicationEngine）。

## 0. 工具链与产物

| 组件 | 路径 | 作用 |
|---|---|---|
| Rust 样本+roundtrip runner | `neo-core/examples/serialization_diff_runner.rs` | 构造样本 → `encode`→字节、`decode`→再 `encode`，断言字节稳定 + `size()` 一致；输出共享向量 |
| C# 交叉验证 runner | `tools/csharp-serialization-runner/Program.cs` (+ `.csproj`) | 读入 Rust 向量，用 **Neo 3.10.1** `decode`+重新`encode`，比对字节 |
| 比对&合并 | `scripts/serdiff_compare.py` | 按向量名交叉引用两侧结果，产出合并摘要 |
| 证据（证据目录）| `reports/formal/serialization-diff-2026-09-18/` | `vectors.rust-encoded.json`（Rust 编码字节）、`rust-roundtrip-results.json`（Rust 内部 roundtrip）、`csharp-3.10.1-crosscheck-results.json`（C# 重编码字节）、`serdiff-merge-summary.json`（合并摘要） |

C# runner 的 csproj 将 `Neo` 固定为 **3.10.1**（与 VM 差异验证同一审计基线，勿浮动）。

## 1. 方法（区分两类证据）

对每个向量执行两件独立的事：

1. **Rust 内部 roundtrip**：`encode(x) → bytes`、`decode(bytes) → x'`、再 `encode(x')`，断言
   `encode(x') == bytes` 且 `size() == bytes.len()`。这是"Rust 自己能 roundtrip"的证据。
2. **与 C# 字节级交叉**：把 Rust `encode` 出的字节交给 C# Neo 3.10.1 解码，C# 重新序列化，
   断言 C# 重编码字节 == Rust 字节。这是"Rust 编码与 C# 参考实现字节一致"的证据。

由于两侧**编码字节本身逐字节相等**（证据 2），且 Rust 对同一批字节可稳定 roundtrip（证据 1），
"C# 编码 → Rust 解码"方向与正向等价，无需单独再跑（这是由字节恒等推出的结论，非单独执行）。

## 2. 样本规模（42 个向量）

| 类别 | 数量 | 覆盖内容 |
|---|---|---|
| `varint` | 11 | 全部编码宽度边界：0,1,252,253,254,255,65535,65536,`u32::MAX`,2³²,`i64::MAX`（1/3/5/9 字节） |
| `varbytes` | 9 | 长度前缀边界：len 0,1,251,252,253,254,255,65534,65535 |
| `header` | 7 | 基本头、空 witness、witness 脚本长度 1/252/253/254/1024（`deserialize` 上限 1024 内的 varint 边界） |
| `transaction` | 9 | 最小交易、3 签名者+3 witness、**5 种属性齐备**（HighPriority/OracleResponse/NotValidBefore/Conflicts/NotaryAssisted）、**自定义 scope 签名者**（CustomContracts+CustomGroups+WitnessRules，含嵌套 WitnessRule/BooleanCondition、压缩 EC 点）、script 长度 1/252/253/254/65535 |
| `block` | 6 | 0 交易、1 交易、3 交易、252/253/256 交易（交易数 varint 边界：252 用 1 字节、253 起用 `0xFD,0xFD,0x00`） |

字段顺序覆盖：header 的 `version/prev_hash/merkle_root/timestamp/nonce/index/primary_index/next_consensus/witness`；
transaction 的 `version/nonce/system_fee/network_fee/valid_until_block/signers/attributes/script/witnesses`；
signer 的 `account/scopes`（可选 `allowed_contracts/allowed_groups/rules`）；attribute 的类型字节 + 载荷；
witness 的变长双脚本；UInt160/UInt256 固定 20/32 字节；所有数组用 varint 长度前缀。

## 3. 结果

| 证据 | 结果 |
|---|---|
| Rust 内部 roundtrip | **42/42 PASS**（`encode→decode→encode` 字节稳定，`size()` 与实长一致），0 失败 |
| C# 3.10.1 字节交叉 | **42/42 字节一致**（C# 解码 Rust 字节后重编码 == Rust 字节），0 不一致，0 解码异常 |
| 合并交叉引用（`serdiff_compare.py`） | `byte_compatible=true`，两侧名称集合完全重合（无缺失、无多余） |

样本入口摘录（`tx_signers_custom_scopes`）可在此文件与 `serdiff-merge-summary.json` 中核对：
C# 重编码十六进制与 Rust 字节逐项一致，包括 `0x10` CustomContracts、`0x20` CustomGroups（33 字节压缩 EC 点）、
`0x40` WitnessRules 布尔条件、以及复合 scope 的组合编码。

## 4. 诚实声明：边界与未覆盖

这是 **wire 序列化层**的差异验证，验证的是"字节流一致性"，**不等于区块状态一致**。

- **已证明**：对 42 个代表性样本，Rust 的 `Block/Transaction/Header`(及其签名者/属性/witness/varint/varbytes 子结构)
  与 C# Neo 3.10.1 在**字节级**上双向一致；Rust 内部可稳定 roundtrip。
- **未覆盖（边界，必须声明）**：
  - **样本是合成样本**，未覆盖真实主网的每个交易形态；未覆盖 `OracleResponse` 的**大结果/超长变长载荷**，以及
    `NotaryAssisted` 之外个别属性类型的极端取值组合。
  - `WitnessCondition` 只实测了 `Boolean`，**未覆盖**其索引/与/或/ScriptHash/Group/CalledByContract/CalledByGroup/
    Not 等复合条件嵌套的字节编码（serialization 逻辑存在，但本差异向量未遍历）。
  - `WitnessRule`/`WitnessCondition` 的**反序列化深度上限**、signer 子项上限（16）、
    attribute/signer 计数上限、script ≤65535 及**超限时（如 script=65536）的负向拒绝路径**仅在 Rust 侧有
    既有的单元测试覆盖，本差异向量**未做负向**（只做了合法样本的往返）。
  - **跨进程的真实 C# 编码 → Rust 解码**未单独作为一次"反向执行"跑（由字节恒等推出等价，见 §1）；
    若要求程序化的双向独立执行，可再加一步：把 C# 重编码字节喂回 `serialization_diff_runner` 的 decode 分支。
  - 本验证不涉及状态层：**ApplicationEngine 语义、原生合约、存储/MPT、GAS 记账、哈希与 witness 的签名校验语义**
    均不在此次 wire roundtrip 差异测试内（归属 VM/状态差异验证的后续目标，见
    `reports/formal/vm-diff-current-vs-3.10.1-2026-09-18.md` 的缺口清单）。

新增的文件：
- `neo-core/examples/serialization_diff_runner.rs`（Rust 样本+roundtrip+向量导出）
- `tools/csharp-serialization-runner/Program.cs`、`tools/csharp-serialization-runner/csharp-serialization-runner.csproj`（C# 交叉验证）
- `scripts/serdiff_compare.py`（合并比对）
- `reports/formal/serialization-diff-2026-09-18/`（证据）

未改动其他代理在改的 syscall 差异 / Coq 模型文件；未 commit/push。

## 5. 复现命令

```bash
# Rust：构造样本，跑内部 roundtrip，导出向量
cargo build -p neo-core --example serialization_diff_runner
cargo run -q -p neo-core --example serialization_diff_runner -- .cache/serdiff/vectors.json .cache/serdiff/rust-results.json

# C# Neo 3.10.1：解码 + 重编码比对
dotnet run --project tools/csharp-serialization-runner -- .cache/serdiff/vectors.json .cache/serdiff/csharp-results.json

# 合并比对（exit 0 = 全部一致）
python scripts/serdiff_compare.py .cache/serdiff/rust-results.json .cache/serdiff/csharp-results.json \
  reports/formal/serialization-diff-2026-09-18/serdiff-merge-summary.json
```