# 真实 MainNet 签名验证差分 vs C# Neo 3.10.1 — 2026-09-20

## 结论

**Rust 与 C# 3.10.1 对真实 MainNet 交易的签名验证结果完全一致（534/534）**。

- 来源：`neo-core/tests/protocol_compliance/test_vectors/mainnet_blocks.json`
  （27 个里程碑区块、534 笔交易，取自 seed1.neo.org，network=mainnet magic=860833102）
- 每笔交易取其标准签名 witness，构造签名消息 `sign_data = network(magic, 4 LE) || tx_hash(32)`，
  用两侧各自的 secp256r1 实现验签。
- Rust 侧（neo-crypto Secp256r1Crypto）：**534/534 验证通过**，exit 0
- C# 侧（Neo 3.10.1 `Crypto.VerifySignature`，即 C# 节点 witness 验证用的同一实现）：
  **534/534 验证通过**，exit 0
- 双侧逐签名对拍：**534/534 结果一致**（compare exit 0）

## 覆盖意义

- **签名消息构造**（magic LE || 容器 hash）双侧一致 —— 这是 CheckSig/CheckMultisig 的输入基础；
- **secp256r1 ECDSA + SHA256 验签**两侧一致 —— witness 有效的核心密码原语；
- **压缩公钥解码**（33 字节 0x02/0x03 前缀 → 曲线上点）两侧一致；
- 用的是**真实链上数据**，非自造向量 —— 直接对生产交易有效。

## 方法说明与诚实边界

- 完整 `Helper.VerifyWitnesses`（含 VM 执行 verification script + 原生合约存储读取）
  在 C# 3.10.1 **公开 API 下不可复现**：`ApplicationEngine.Create` 需要链上创世初始化存储
  （Ledger/ContractManagement 原生合约记录），空快照下抛 `KeyNotFoundException`（键 id=-4/prefix=0x0C
  已探明但原生合约本体未部署，随后 VM 抛 `Null`→`Struct` 转换错误）。这在公开 API 边界内无解，
  故降档到**签名原语层**。
- 本差分**不覆盖**：自定义 verification script（多签、自定义条件脚本）、WitnessRule、
  oracle 响应、gas 计费、storage 读写合约的 witness（需链上状态）。这些属完整的区块处理/状态根轨道
  （Phase 5）而非纯签名层。
- 但签名 witness 是 N3 交易的绝对主流形态（本向量集 534/534 都是），其核心有效性
  （签名本身 + 消息构造 + 公钥）已在此差分中与 C# 权威实现闭合。

## 复现

```bash
# Rust 侧：生成 per-witness 验签结果 + sign_data/pubkey/signature
cargo run -q -p neo-core --example signature_verify_diff_runner -- \
  neo-core/tests/protocol_compliance/test_vectors/mainnet_blocks.json \
  reports/formal/signature-verify-rust.json

# C# 侧：用 Neo 3.10.1 权威 Crypto.VerifySignature 重验
dotnet run --project tools/csharp-signature-runner -- \
  reports/formal/signature-verify-rust.json reports/formal/signature-verify-csharp.json

# 对比
python scripts/signature_verify_diff_compare.py \
  reports/formal/signature-verify-rust.json reports/formal/signature-verify-csharp.json \
  reports/formal/signature-verify-merge.json
```
