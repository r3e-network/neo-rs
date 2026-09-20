# 形式化/差分验证轨道总计划 — 2026-09-20

目标：回答"neo-rs Rust 节点是否完全正确实现 Neo N3 协议"。策略 = 分层差分，逐层闭合字节级/原语级证据，运行时集成留作后续。

## 证据矩阵（当前状态）

| 层 | 轨道 | 状态 | 证据 |
|---|---|---|---|
| 工作区基线 | Phase 0 | ✅ 完成 | 206 套件 / 4245 通过 / 0 失败 / exit 0 |
| VM 执行语义 | 既有 | ✅ 完成 | 99 纯 VM 向量 + 708 MainNet 链上向量 vs C# Neo.VM 3.10.1 |
| 序列化线格式 | 既有 | ✅ 完成 | 42 向量 Block/Header/Tx/varint/varbytes 字节级一致 |
| P2P 线格式 | Phase 1 | ✅ 完成 | 9 向量 + 21 命令字节 vs C# 3.10.1（9/9 + 21/21） |
| 签名原语 | Phase 2 | ✅ 完成 | 534 真实 MainNet 签名 vs C# 3.10.1（534/534 一致） |
| 纯函数规格 | 既有 | ✅ 完成 | 307 项 Prusti 189 + Verus 118 |
| Coq 模型 | 既有 | ✅ 完成 | 34/34 全绿门禁 |
| dBFT 互操作冒烟 | Phase 3 | ⏸ 待运行时基建 | 需验证人钱包 + 多进程 + 存储隔离 |
| RPC 读接口差分 | Phase 4 | ⏸ 待运行时基建 | 需起 RPC 服务对比返回 JSON 形状 |
| 端到端状态根差分 | Phase 5 | ⏸ 长期轨道 | 需与 C# 节点并行同步比对状态根 |

## 后续轨道（需节点运行时工程）

**Phase 3 — dBFT 私网冒烟**：dbft 服务已接线（`neo-node/src/consensus.rs` 的
`DbftConsensusController` + `WalletConsensusSigner`，可经 RPC `startconsensus` 启动，
`auto_start` 可配）。前置：验证人钱包（持有私钥、未锁定）、`ValidatorInfo`
（index/pubkey/script_hash）、≥2 进程、存储隔离、统一网络配置。这是独立集成任务。

**Phase 4 — RPC 读接口差分**：返回 JSON 字段名/形状（`getblock` 等）需起 RPC 服务后与
C# RpcServer 对比。离线无字节级可比（JSON-RPC 是标准 JSON）。

**Phase 5 — 端到端状态根差分**（最高价值，最长期）：与 C# 节点并行同步同一批 MainNet
区块，逐块比对状态根。这是"完全正确实现"的最强单条证据。

## 本轮已闭合的字节/原语证据

1. **P2P 线格式**（Phase 1，`reports/formal/p2p-wire-diff-2026-09-20.md`）
2. **签名原语**（Phase 2，`reports/formal/signature-diff-2026-09-20.md`）
3. 配合既有序列化/VM 差分，neo-rs 与 Neo 3.10.1 在**无状态字节层与签名密码层**已字节级/逐签名一致。

## 诚实边界

本矩阵闭合的是"无状态字节层 + 签名密码层 + 纯函数 + 抽象模型"。**尚未闭合**：完整 witness
验证（依赖链上原生合约存储）、区块处理状态迁移、共识互操作、RPC 形状、状态根。这些属
Phase 3/4/5 的运行时工程，未到可以声称"完全正确实现"的程度。
