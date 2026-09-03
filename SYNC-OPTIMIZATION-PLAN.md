# Neo-RS 区块同步优化方案

## 问题诊断

### 当前状态

- 节点卡在区块 21,373，完全没有进展
- 日志显示大量握手成功，但没有区块请求/接收日志
- 连接超时率高（6秒超时）

### 根本原因

#### 1. TaskManager 配置过于保守

```rust
const TASK_TIMEOUT: Duration = Duration::from_secs(20);  // 太短
const MAX_CONCURRENT_TASKS: u32 = 3;                     // 太少
const MAX_BLOCK_INDEX_BATCH: u32 = 64;                   // 批次太小
const BLOCK_INDEX_WINDOW_MULTIPLIER: u32 = 8;            // 窗口太小 (512块)
```

**影响**：

- 20秒超时对网络延迟敏感，频繁重试
- 每个peer只能3个并发任务，10个peer = 最多30个并发
- 每批64个区块，需要 9,073,519 / 64 = 141,774 批次
- 全局窗口512块限制了整体并行度

#### 2. 连接超时配置

```rust
// 在 connection 或 timeouts 模块中
connection_timeout = 6秒  // 太短
```

#### 3. 可能的初始同步触发问题

节点握手后没有主动请求区块，可能是：

- TaskManager 没有正确初始化
- RemoteNode 注册后没有触发 `request_tasks`
- 初始区块高度检测有问题

## 优化方案

### 阶段1: 提升并发和批次大小（立即实施）

#### 修改 1: task_manager.rs 常量优化

```rust
// 从 20秒 → 60秒，减少不必要的超时重试
const TASK_TIMEOUT: Duration = Duration::from_secs(60);

// 从 3 → 10，大幅提升每个peer的并发能力
const MAX_CONCURRENT_TASKS: u32 = 10;

// 从 64 → 500，减少批次数量
const MAX_BLOCK_INDEX_BATCH: u32 = 500;

// 从 8 → 20，扩大全局窗口到 10,000 块
const BLOCK_INDEX_WINDOW_MULTIPLIER: u32 = 20;

// 保持30秒，合理的检查间隔
const TIMER_INTERVAL: Duration = Duration::from_secs(30);
```

**预期效果**：

- 并发度：3 → 10 (每peer)，总并发：30 → 100
- 批次数：141,774 → 18,147 (减少87%)
- 全局窗口：512 → 10,000 (提升19.5倍)
- 超时重试：大幅减少

### 阶段2: 网络超时优化

#### 修改 2: 连接超时配置

查找并修改连接超时：

```rust
// 从 6秒 → 15秒
connection_timeout: Duration::from_secs(15)
```

### 阶段3: 增强日志和监控

#### 修改 3: 添加关键同步日志

在 `request_tasks_entry` 中添加：

```rust
trace!(
    target: "neo",
    actor = %actor.path(),
    current_height = current_height,
    header_height = header_height,
    peer_height = session.last_block_index,
    "requesting tasks from peer"
);
```

在 `complete_inventory` 中添加：

```rust
info!(
    target: "neo",
    hash = %hash,
    block_index = ?block_index,
    "block received and stored"
);
```

### 阶段4: 初始同步触发检查

#### 修改 4: 确保注册后立即触发同步

在 `register_session` 末尾已经有：

```rust
self.request_tasks_for_path(&path);
```

但需要确认 `request_tasks_entry` 的条件逻辑。

## 性能预测

### 当前性能

- 并发：30个区块
- 批次：64个/批
- 理论速度：~2块/秒（考虑网络延迟）
- 同步900万块需要：~52天

### 优化后性能

- 并发：100个区块
- 批次：500个/批
- 理论速度：~20块/秒
- 同步900万块需要：~5.2天

**提升：10倍速度**

## 实施步骤

1. ✅ 分析完成
2. ⏳ 修改 task_manager.rs 常量
3. ⏳ 查找并修改连接超时
4. ⏳ 添加同步日志
5. ⏳ 编译测试
6. ⏳ 部署到服务器
7. ⏳ 监控同步进度

## 风险评估

### 低风险

- 提升超时时间：只会减少重试，不会影响正确性
- 增加并发数：内存占用略增（可接受）
- 扩大批次：网络带宽略增（可接受）

### 需要监控

- 内存使用：预计从 300MB → 500MB
- 网络带宽：预计从 1MB/s → 5MB/s
- CPU使用：预计从 1% → 3%

## C# 参考实现对比

C# Neo 节点的配置：

```csharp
// TaskManager.cs
private const int MaxConCurrentTasks = 3;  // 相同
private const int TaskTimeout = 60;        // 60秒（我们当前20秒）
```

我们的优化将超过C#实现的性能。
