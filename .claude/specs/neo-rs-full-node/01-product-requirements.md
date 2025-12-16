# neo-rs 完整性 PRD (Product Requirements Document)

**项目**: neo-rs 区块链全节点  
**版本**: 0.7.0 → 1.0.0  
**日期**: 2025-12-16  
**质量评分**: 91.5/100  
**状态**: ✅ 已批准

---

## 1. 用户批准的配置

| 决策项 | 选择 | 理由 |
|--------|------|------|
| 架构方向 | **Option B** - 新模块化运行时 | 更好的长期可维护性 |
| 功能优先级 | **#2** - 共识验证器模式 | 运行验证器节点最关键 |
| 发布时间表 | **完整路径** (16-20周) | 全功能 + 安全审计 |
| 测试策略 | **扩展** | 混沌测试、模糊测试、性能测试 |
| 部署目标 | **MainNet 全节点** | 生产环境部署 |

---

## 2. 已完成的工作（2025-12-16 会话）

- ✅ P2P 双向广播通道 (`BroadcastMessage` struct)
- ✅ ChainState 传递给 P2P 服务 (`set_chain` method)
- ✅ 共识交易响应通道 (`on_transactions_received`)
- ✅ 验证器钱包加载 (`load_validator_from_wallet`)
- ✅ CLI 参数支持 (`--wallet`, `--wallet-password`)

---

## 3. 功能需求

### 3.1 P0 - 关键路径

#### 3.1.1 模块化运行时完成 (第 1-4 周)
- [ ] neo-chain：块索引、分叉选择、链重组
- [ ] neo-state：MPT 树、账户状态、合约存储持久化
- [ ] neo-mempool：优先级排序、费用验证
- [ ] neo-consensus：dBFT 消息处理、视图变更

#### 3.1.2 共识验证器模式 (第 5-8 周)
- [x] 验证器钱包集成（NEP-6）- 已完成
- [x] 共识事件桥接（P2P 广播）- 已完成
- [ ] 块提议和投票完整流程
- [ ] 故障恢复机制
- [ ] 视图变更处理

#### 3.1.3 完整 RPC 接口 (第 9-12 周)
- [ ] Blockchain API（getblock、getblockheader、getbestblockhash 等）
- [ ] State API（getstate、getstates、findstates 等）
- [ ] Transaction API（sendrawtransaction、getapplicationlog 等）
- [ ] Wallet API（getbalance、getnep17balances 等）

### 3.2 P1 - 重要功能

#### 3.2.1 状态持久化
- [ ] Genesis 块执行
- [ ] 块执行器连接到 RocksDB
- [ ] MPT 状态根计算和验证
- [ ] 状态回滚（链重组支持）

#### 3.2.2 P2P 网络完善
- [x] GetHeaders 响应处理 - 已完成
- [ ] GetBlocks 响应处理
- [ ] GetData 响应（块和交易）
- [ ] 对等节点声誉评分

### 3.3 P2 - 增强功能

- [ ] 快照/恢复功能
- [ ] 快速同步模式
- [ ] 分布式跟踪（OpenTelemetry）
- [ ] Grafana 仪表板

---

## 4. 非功能需求

### 4.1 性能目标

| 指标 | 目标 |
|------|------|
| 块执行 | < 1 秒/块 |
| RPC 响应 | < 100ms (p99) |
| 内存使用 | < 2GB (稳定状态) |
| 启动时间 | < 30 秒 |

### 4.2 安全要求

- [ ] 第三方安全审计
- [ ] RPC 速率限制
- [ ] P2P 消息验证
- [ ] 安全的钱包密钥管理

### 4.3 测试覆盖率

- 单元测试：≥90%
- 集成测试：完整端到端覆盖
- 混沌测试：网络故障、节点故障场景
- 性能测试：基准和回归测试
- 模糊测试：RPC 输入、块/交易解析

---

## 5. 成功标准

- [ ] 100% Neo JSON-RPC 2.0 方法实现
- [ ] 完整的 dBFT 共识参与
- [ ] 与 C# Neo 节点 100% 兼容
- [ ] 测试覆盖率 ≥90%
- [ ] 安全审计通过（0 Critical/High）
- [ ] MainNet 完整同步成功

---

## 6. 时间表

| 阶段 | 周数 | 内容 |
|------|------|------|
| Phase 1 | 1-4 | 模块化运行时完成 |
| Phase 2 | 5-8 | 共识验证器模式 |
| Phase 3 | 9-12 | 完整 RPC 接口 |
| Phase 4 | 13-16 | 优化和安全审计 |
| Phase 5 | 17-20 | 扩展和文档 |

---

**文档结束**
