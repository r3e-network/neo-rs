# 区块/交易验证规则差分工具

## 目标
验证 neo-rs 的区块/交易验证规则与 Neo N3 3.10.1 完全一致。

## 测试向量
- **区块**：50 个（20 个有效 MainNet 区块 + 30 个无效变种）
- **交易**：500 个（400 个有效 MainNet 交易 + 100 个无效变种）

## 工具链

### 1. rust_validator.rs
包装 neo-protocol 的验证规则：
```rust
use neo_protocol::block::Block;
use neo_protocol::transaction::Transaction;

pub fn validate_block(block: &Block) -> ValidationResult {
    // 调用 neo-protocol 验证规则
}

pub fn validate_transaction(tx: &Transaction) -> ValidationResult {
    // 调用 neo-protocol 验证规则
}
```

### 2. csharp_validator.cs
包装 Neo 3.10.1 的验证规则：
```csharp
using Neo;
using Neo.Network.P2P.Payloads;

public ValidationResult ValidateBlock(Block block) {
    // 调用 Neo.Block.Verify()
}

public ValidationResult ValidateTransaction(Transaction tx) {
    // 调用 Neo.Transaction.Verify()
}
```

### 3. diff_runner.py
差分比对器：
```python
def run_validation_diff():
    # 1. 加载测试向量
    # 2. Rust 侧验证
    # 3. C# 侧验证
    # 4. 比对结果
    # 5. 生成报告
```

## 验证点

### 区块验证
- `validate_block_header()` vs C# `Block.Verify()`
  - timestamp 边界检查
  - version 检查
  - primary_index < validator_count
  - nonce 唯一性
  - merkle_root 计算

### 交易验证
- `validate_transaction()` vs C# `Transaction.Verify()`
  - size <= MAX_TRANSACTION_SIZE
  - system_fee 计算
  - network_fee 足够
  - attributes 合法性
  - witnesses 数量与签名者匹配

## 当前 Rust 输入与验证语义

每个区块向量必须且只能提供 `block_hex`、`block_base64`、`block_json` 之一；交易向量对应使用 `tx_hex`、`tx_base64`、`tx_json`。hex 允许 `0x` 前缀，JSON 使用 Neo RPC block/transaction 对象。

二进制输入由规范 `Block`/`Transaction` 反序列化器严格解码，尾随字节会被拒绝。区块执行真实结构验证；交易在结构验证后继续执行 `verify_transaction_state_independent`，包含真实见证验证。无效输入和验证失败分别保留错误类型与错误信息，批处理不会因单个坏向量中止。

当前交易检查是**状态无关验证**，不包含依赖链上快照的余额、费用、策略或冲突检查。README 中的数量是差分测试目标，不是已经完成的 C# parity 结果。

## 成功标准
- 50/50 区块验证结果与 C# 一致
- 500/500 交易验证结果与 C# 一致
- 错误类型（accept/reject reason）完全匹配

## 输出
`reports/formal/validation-diff-report.md`
