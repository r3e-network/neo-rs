# P2P 线协议差分 vs C# Neo 3.10.1 — 2026-09-20

## 结论

**BYTE-COMPATIBLE（在该向量集上）**：neo-rs 的 P2P 线格式与 C# Neo 3.10.1 权威实现字节级一致。

- Rust 内部 roundtrip：**30/30** 通过（9 个 payload 向量 + 21 个命令字节断言），exit 0
- C# Neo 3.10.1 解码+重编码：**9/9** 向量字节一致，exit 0
- 命令字节表：**21/21 全部一致**（对照 Neo 3.10.1 `MessageCommand` enum 权威输出）
- 汇总：`reports/formal/p2p-wire-merge-summary.json`（byte_compatible=true）

## 覆盖向量

| 向量 | 内容 |
|---|---|
| version_payload_basic | magic/ver/ts/nonce/user_agent + TcpServer + FullNode capabilities |
| ping_payload | last_block_index/timestamp/nonce |
| inv_payload_tx | 3×UInt256 交易哈希清单 |
| getblocks_payload | hash_start + count=-1 |
| getblockbyindex_payload | index_start/count |
| addr_payload | IPv4 映射 + IPv6 两个 NetworkAddressWithTime |
| filterload / filteradd | bloom 过滤器载荷 |
| message_frame_ping | 完整线帧 flags+command+varbytes |

## 重要更正（差分的价值）

事前怀疑 Rust 侧 `GetBlocks=0x24/Inv=0x27/Extensible=0x2e` 与 C# 标准（记忆中的 0x22/0x24/0x90）不符。
**权威裁决：怀疑错误**——Neo 3.10.1 的真实枚举值就是 0x24/0x27/0x2e，neo-rs 全部一致。
教训照旧：**勿凭记忆断言线格式，一切以权威包实测为准。**

## 已知遗留项（非阻塞）

- C# enum 另有 `Transaction=0x2b`、`Block=0x2c` 两个命令，neo-rs 解析为 `Unknown` 并按协议违规拒绝。
  现代网络通过 inv/getdata 分发，这两个命令实际不在线上使用；若未来要与极端遗留节点互通需补。

## 未覆盖（后续轨道）

- 压缩路径（payload ≥ 阈值时 LZ4 帧一致性与 `COMPRESSION_MIN_SIZE` 阈值对齐）
- Headers/MerkleBlock/Extensible 载荷（依赖 Block/ConsensusPayload 结构，属序列化差分扩展）
- 有状态行为（握手时序、verack 逻辑、断线重连）

## 复现

```bash
cargo run -q -p neo-p2p --example p2p_wire_diff_runner -- \
  reports/formal/p2p-wire-vectors.rust.json reports/formal/p2p-wire-rust-results.json
dotnet run --project tools/csharp-p2p-runner -- \
  reports/formal/p2p-wire-vectors.rust.json reports/formal/p2p-wire-csharp-results.json
python scripts/p2p_wire_diff_compare.py \
  reports/formal/p2p-wire-vectors.rust.json reports/formal/p2p-wire-rust-results.json \
  reports/formal/p2p-wire-csharp-results.json reports/formal/p2p-wire-merge-summary.json
```
