# neo-rs 全量项目审计报告

- **审计日期**：2026-09-07
- **修复完成核对**：2026-09-07（本文件）
- **对象**：`neo-rs` @ 分支 `protocol-v3.10.1-compliance`
- **协议目标**：Neo N3 v3.10.1 / workspace `0.17.0`

## Overview

**结论：审计清单中的可落地代码/配置缺陷均已修复并附回归或编译验证。** Iteration 2 额外修复了 A18 引入的 primary 错 hash 计票缺口、Commit 广播路径统一、UPnP SSRF（拒 IPv4 link-local / 禁 redirect）、畸形 RPC 限流旁路、PrepareResponse 提前缓冲、BlockLike/TEE ACL/Zeroizing 收尾。唯一保留的非阻断残留是主网 C#↔Rust 差分回放未跑，以及 advisories 步骤 `continue-on-error`。Windows TEE 宿主文件 ACL 已通过 `icacls` owner-only DACL 落地（见 `docs/tee-fs-acl.md`）。

### Remediation matrix

| ID | 状态 | 证据 |
|----|------|------|
| A01 | **已修** | `recovery.rs` + `can_sign_commit`；测试 `recovery_without_prepare_request_does_not_sign_zero_hash_commit` |
| A02 | **已修** | `ledger_contract/mod.rs` 已从 HEAD 恢复（非空模块） |
| A03 | **已修** | 公网无 auth → `bail!`；测试 `validate_rejects_public_bind_without_auth` |
| A04 | **已修** | TEE 计数器 fail-closed；`test_corrupt_monotonic_counter_fails_closed` |
| A05 | **已修** | `new_with_preloaded_native` → `select_hardfork_vm_semantics` |
| A06 | **已修** | `create_payload` 签名失败返回 `Err` |
| A07 | **已修** | `/ws` BasicAuth；401 |
| A08 | **已修** | `openwallet` 路径 jail（`WalletDirectory` / CWD） |
| A09 | **已修** | `max_sessions` + `store_session` → `Result` / `-609` |
| A10 | **已修** | RPC signer `MAX_SIGNER_SUBITEMS` |
| A11 | **已修** | 仅在 `process_object` 按 method tier 计费一次；tier `/5`/`/10` 下限 1 |
| A12 | **已修** | reputation 低于阈值 → `ban_peer` |
| A13 | **已修** | 账本存在性/冲突 fail-closed |
| A14 | **已修** | RocksDB 默认 `WriteBatchConfig::durable()` |
| A15 | **已修** | 主机密封密钥需 `NEO_TEE_ALLOW_HOST_SEALING_KEY=1` |
| A16 | **已修** | release 模拟 TEE 需 `NEO_TEE_ALLOW_SIMULATION=1` |
| A17 | **已修** | `deny.toml` + CI `cargo-deny`（licenses/bans/sources 硬门禁） |
| A18 | **已修** | prepare hash 绑定 + quorum 仅计匹配 hash |
| A19 | **已修** | 错误 BasicAuth → HTTP 401 |
| A20 | **已修** | 删除不可核实的 `FINAL-OPTIMIZATION-STATUS.md` |
| A21 | **已修** | `ARCHITECTURE.md` 使用 `neo-vm` |
| A22 | **已修** | 共识签名缓冲 `Zeroizing<[u8;32]>`（`docs/consensus-signing-key-zeroize.md`） |
| R03 | **已修** | Commit 广播前始终持久化 recovery（不受 `ignore_recovery_logs` 跳过） |
| P2 hardened methods | **已修** | 禁用 dump/import/send* |
| P2 CORS default | **已修** | `enable_cors` 默认 false |
| P2 UPnP SSRF | **已修** | LOCATION 仅私网 IP |
| P2 mTLS roots | **已修** | TrustedAuthorities 无匹配 → Err |
| P2 verifyproof | **已修** | `-607` invalid_proof |
| P2 docs TLS | **已修** | `RPC_HARDENING.md` |
| P2 hash() zero | **已修** | serialize 失败 panic（fail-closed） |
| P2 oracle tx id | **已修** | 无 Transaction 容器 → Err |
| P2 HashIndexState | **已修** | 非法栈值 → Err |
| P2 diagnostic VM | **已修** | 诊断不再禁用 fast path |
| P3 BasicAuth timing | **已修** | 定长缓冲 + 长度混入 ct_eq |
| P3 HSM PIN | **已修** | `Zeroizing<String>` |
| P3 TEE sealing key | **已修** | `Zeroizing<[u8;32]>` |
| P3 Windows TEE ACL | **已修** | `neo-tee/src/fs_acl.rs` + `docs/tee-fs-acl.md`；测试 `restrict_owner_only_roundtrip` |
| 主网差分回放 | **残留（范围外）** | 协议等价证明项，非本仓库缺陷修复门禁 |

### Design / Test notes

- 共识 fail-closed：recovery / create_payload / check_commits / Commit 持久化门闩 / 签名 key Zeroizing 一致。
- RPC 默认更收紧：CORS off、公网需 auth、wallet jail、session 上限、hardened 扩禁用面。
- 存储默认 durable（`sync_on_flush=true`）；高吞吐仍可显式切换。
- TEE 宿主敏感文件：Unix `0600`/`0700`；Windows `icacls` owner-only。
- `cargo deny check licenses bans sources` 本地已通过；advisories 依赖 advisory-db 网络，CI 单独步骤且 `continue-on-error`，已知 RUSTSEC 记在 `deny.toml` ignore。

### Iteration 2 (2026-09-07 evening) — audit / optimize / refactor

| ID | 状态 | 说明 |
|----|------|------|
| I2-01 | **已修** | A18 后 `has_enough_prepare_responses` 仍用 `primary_in_responses` 计票：primary 仅有**错误 hash** 的 PrepareResponse 且无本地 PrepareRequest 时会被错误计入 M。改为 `matching + (PrepareRequest ∧ ¬primary_match)`；回归测试 3 条。 |
| I2-02 | **已修** | `check_prepare_responses` / recovery 重复 Commit 广播路径；`prepare` 在 `can_sign_commit` 后仍 `unwrap_or_default()`。抽取 `try_broadcast_own_commit`（幂等、fail-closed）。 |
| I2-03 | **已修** | UPnP LOCATION：`Url::host()`；拒 IPv4 link-local（含 `169.254.169.254`）；禁 redirect；control URL 再闸。 |
| I2-04 | **已修** | 测试对齐 A06/A18：真实签名密钥；future-view PrepareResponse 缓冲语义。 |
| I2-05 | **已修** | 畸形 RPC / notification / 缺 method 按 Standard 计费，合法 method 仍只计 method tier（修 A11 引入的 DoS 旁路）。 |
| I2-06 | **已修** | `BlockLike::hash` fail-closed panic（与 Header/Tx 一致）。 |
| I2-07 | **已修** | Windows TEE ACL：先 grant 再 strip inheritance，再 re-grant。 |
| I2-08 | **已修** | `sealing_key()` 返回 `Zeroizing<[u8;32]>`。 |
| I2-09 | **已修** | 未知 `preparation_hash` 时缓冲已验签 PrepareResponse；PrepareRequest 到达后 retain 匹配并 `check_prepare_responses`。 |

验证：`neo-consensus` 114；`neo-core` upnp + block；`neo-tee` 56。

### Residual (intentional / out of scope)

1. `cargo-deny` advisories 步骤 `continue-on-error`（advisory-db 网络）；licenses/bans/sources 仍硬门禁。
2. 主网状态根差分回放门禁（见 `docs/protocol-consistency/STATUS.md`）。I3-01 修复了 980196 的高嫌疑根因，但仍需本地 full-state DB 回放确认。
3. 升级 warp 0.4 / hyper 1.x / rand 0.9+ 栈以清空 RUSTSEC ignore（大迁移，未在本轮落地）。

### Iteration 3 (2026-09-07) — protocol / CheckWitness

| ID | 状态 | 说明 |
|----|------|------|
| I3-01 | **已修（待 DB 回放确认）** | `process_pending_native_calls`：多个 pending 回调曾互相作为 `calling_context`，破坏 `CalledByEntry`（NEO.transfer 后 GAS mint 再排队 `onNEP17Payment`）。所有回调现绑定 native-invoke 帧。回归：`pending_native_callbacks_share_native_invoke_calling_context`。 |
