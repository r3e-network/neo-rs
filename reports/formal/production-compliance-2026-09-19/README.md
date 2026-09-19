# Neo-rs 生产部署 / 合规 / 事故响应 / 安全验收材料

> **日期**：2026-09-19
> **协议基线**：Neo N3 v3.10.1
> **目标（原计划）**：Production deployment preparation (compliance checklists, incident response updates)。
> **本包定位**：以 `reports/formal/` 下的**形式验证 / 差异验证证据**为纲，产出部署、合规、事故响应、安全验收四份交付物。**核心原则：已证证据与未验证盲区并列，作为准入依据；不宣称"已生产就绪"。**

---

## 交付物清单

| 文件 | 主题 | 要点 |
|------|------|------|
| [DEPLOYMENT.md](./DEPLOYMENT.md) | 部署清单（证据为纲） | 验证门槛 G1-G5、阶段 A/B/C、盲区 S1-S6 |
| [COMPLIANCE.md](./COMPLIANCE.md) | 合规核查表 | 密钥/凭据纪律、备份恢复、审计、网络、供应链、协议合规，可勾选 |
| [INCIDENT-RESPONSE.md](./INCIDENT-RESPONSE.md) | 事故响应 | 4 个真实场景（状态不一致/coinbase-存储/共识宕机/门禁分歧），含盲区对照与复盘模板 |
| [SECURITY-ACCEPTANCE.md](./SECURITY-ACCEPTANCE.md) | 安全验收 | Mimosa 门禁 + 凭据约束 + 历史脚本高危清理 + 验收决策矩阵 |

---

## 证据基线（简表）

| 证据 | 结果 | 覆盖 | 盲区 |
|------|------|------|------|
| VM 差异验证（`vm-diff-*.md`） | 99 + 708 PASS, 0 different | VM 指令执行语义（bare engine） | 无宿主/syscall/存储 |
| Wire 序列化（`serialization-wire-diff-*.md`） | 42/42 PASS + 42/42 字节一致 | Block/Tx/Header 字节级 | 合成样本，部分负向/复合未覆盖 |
| Syscall/native（`syscall-diff-expanded-*.md`） | 27/27 对齐 PASS | 带宿主 syscall 子集（GasLeft 精确等） | 存储/原生正路径、容器 hash 对齐未完成 |
| Coq 门禁（`coverage-status-2026-09-18.md`） | 25/34 通过 | 抽象模型/契约 | 9 个未通过；非 Rust 精化证明 |

---

## 生产就绪关键盲区（速查）

1. **S1** 存储 Get/Put/Delete/Find 正路径 —— 未验证。
2. **S2** 原生合约价值转移 / genesis 原生态对齐 —— 部分覆盖，有真实解析差异（Rust native registry vs C# deployed ContractState）。
3. **S6** 完整区块状态一致（MPT/state root/原生余额）—— 无证据。
4. **S5** Coq 9 个未通过模型（含 witness/多签/state_root）—— 未证。
5. **S3/S4** 带签名交易跨进程反向 roundtrip、opcode price 表 vs C# —— 未单独执行 / 待交叉校验。

> 含义：当前证据支撑**跟随/观测节点（阶段 A）**；**独立验证者（阶段 B）与权威/分叉裁决（阶段 C）被上述盲区阻断**，补齐取得机器证据后方可放行。

---

## 文档间引用关系

```
DEPLOYMENT.md (证据门槛 G1-G5 / 盲区 S1-S6)
   ├── COMPLIANCE.md (运维合规核查表，引用 §3 盲区)
   ├── INCIDENT-RESPONSE.md (场景响应，附录A 盲区×场景对照)
   └── SECURITY-ACCEPTANCE.md (验收决策矩阵引用 G/盲区 + Mimosa)
```

基础参考（不重复其内容）：`docs/SECURITY.md`、`docs/DEPLOYMENT.md`、`docs/DEPLOYMENT-READINESS-REPORT.md`、`docs/DEPLOYMENT-VALIDATION-CHECKLIST.md`。
