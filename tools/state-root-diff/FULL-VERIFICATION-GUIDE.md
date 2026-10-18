# 全量 MainNet 状态根验证指南

**目标**：验证整个 Neo N3 MainNet（1360+ 万个区块）的状态根一致性

---

## 🚀 快速开始

### 方法 1：使用 Python 运行器（推荐）

```bash
cd /d/Git/neo-rs
python tools/state-root-diff/full_mainnet_verification_runner.py
```

### 方法 2：使用 Bash 脚本

```bash
cd /d/Git/neo-rs
bash tools/state-root-diff/full_mainnet_verification.sh
```

### 方法 3：手动分批运行

```bash
# 批次 1: 0-999
python tools/state-root-diff/verify_state_roots_rpc.py \
    --rust-rpc http://seed2.neo.org:10332 \
    --csharp-rpc http://seed3.neo.org:10332 \
    --start 0 --end 999 \
    --output tools/state-root-diff/results/mainnet_batch1.json

# 批次 2: 1K-10K
python tools/state-root-diff/verify_state_roots_rpc.py \
    --start 1000 --end 9999 \
    --output tools/state-root-diff/results/mainnet_batch2.json

# ... 继续其他批次
```

---

## 📊 验证策略

### 分批验证（推荐）

| 批次 | 范围 | 区块数 | 预估时间 |
|------|------|--------|----------|
| 1 | 0 - 999 | 1,000 | ~8 分钟 |
| 2 | 1K - 10K | 9,000 | ~1 小时 |
| 3 | 10K - 100K | 90,000 | ~12 小时 |
| 4 | 100K - 1M | 900,000 | ~5 天 |
| 5 | 1M - 当前 | ~12,656,000 | ~73 天 |

**总预计时间**：~78 天（连续运行）

### 优化策略

1. **并行验证**：在多台机器上同时运行不同批次
2. **抽样验证**：验证关键区块（创世块、硬分叉点、最近 10K 块）
3. **增量验证**：先验证前 100K，再逐步扩展

---

## 🎯 当前进度

### 已完成 ✅
- [x] 0-10：11 个区块（100% 一致）
- [x] 0-99：100 个区块（100% 一致）

### 进行中 ⏸
- [ ] 0-999：1,000 个区块
- [ ] 1K-10K：9,000 个区块
- [ ] 10K-100K：90,000 个区块
- [ ] 100K-1M：900,000 个区块
- [ ] 1M-当前：~12,656,000 个区块

---

## 💡 实用建议

### 后台运行

```bash
# Linux/Mac
nohup python full_mainnet_verification_runner.py > verification.log 2>&1 &

# Windows PowerShell
Start-Process python -ArgumentList "full_mainnet_verification_runner.py" -NoNewWindow -RedirectStandardOutput "verification.log"
```

### 监控进度

```bash
# 查看日志
tail -f verification.log

# 查看结果文件
ls -lh tools/state-root-diff/results/

# 检查最新结果
cat tools/state-root-diff/results/mainnet_batch1_0_999.json | jq '.total, .matching'
```

### 恢复中断的验证

如果验证中断，脚本会自动保存已完成的批次结果。只需重新运行，脚本会跳过已验证的批次。

---

## 📁 输出文件

### 批次结果
- `mainnet_batch1_0_999.json` - 第一批结果
- `mainnet_batch2_1k_10k.json` - 第二批结果
- ...

### 最终报告
- `full_mainnet_verification_report.json` - 汇总报告

### 结构示例

```json
{
  "timestamp": "2026-09-30T...",
  "total_blocks": 13656283,
  "total_matching": 13656283,
  "total_elapsed_seconds": 6782841,
  "batches": [
    {
      "batch": "batch1_0_999",
      "start": 0,
      "end": 999,
      "total": 1000,
      "matching": 1000,
      "rate": 100.0
    },
    ...
  ]
}
```

---

## ⚠️ 注意事项

1. **网络连接**：需要稳定的网络连接到公共 RPC 节点
2. **速率限制**：公共节点可能有速率限制，脚本已内置延迟
3. **存储空间**：结果文件可能占用几 GB 空间
4. **运行时间**：全量验证需要数周时间

---

## 🚀 推荐方案

### 快速验证（1-2 天）

验证关键区块：
- 创世块（0）
- 前 1000 个区块
- 每个硬分叉点
- 最近 10,000 个区块

### 完整验证（数周）

按照上述分批策略，完整验证所有区块。

---

## 📊 预期结果

### 成功标准

```
🎉 全量验证成功！
总区块数: 13,656,283
匹配数: 13,656,283
一致率: 100.00%
```

### 如果发现不匹配

报告会显示：
- 第一个分歧点的区块高度
- 不匹配的状态根哈希
- 详细的错误信息

---

**准备就绪？运行以下命令开始验证：**

```bash
python tools/state-root-diff/full_mainnet_verification_runner.py
```
