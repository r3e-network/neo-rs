# Task #90 独立验证报告：物理迁移与引擎接口

**验证时间**: 2026-11-05  
**验证者**: Chris (QA Agent)  
**工作区**: d:\Git\neo-rs  
**Rust 工具链**: 1.99.1-x86_64-pc-windows-msvc / gnullvm

---

## 执行摘要

**总体判定**: ❌ **FAIL**

物理迁移与引擎接口**未达到**验收标准。发现以下严重问题：
- 44+ 编译错误分布在 neo-core、neo-wallets、neo-primitives、neo-crypto、neo-state-service
- neo-wallets Cargo.toml 依赖严重缺失（7个必需crate未声明）
- neo-core 仍包含大量应迁移模块（smart_contract、ledger、network、persistence、wallets、state_service等）
- 多个文件使用不稳定特性 `std::assert_matches!` 但无 feature flag
- 格式检查发现多处未格式化代码

---

## 1. 全工作区编译验证

### 1.1 cargo build --workspace --all-features

**命令**: 
```powershell
rustup run 1.99.1 cargo build --workspace --all-features
```

**结果**: ❌ **FAIL** (ExitCode: 1)

**关键错误**:

#### 错误 1: blst 汇编编译失败 (MSVC)
```
LINK : fatal error LNK1181: cannot open input file 
'D:\Git\neo-rs\target\debug\build\blst-f92880ec6a1c76e4\out\a623b7e16c0705e4-assembly.o'
```
- **根因**: blst 依赖的 assembly.S 文件无法被 MSVC cl.exe/lib.exe 编译，需要 clang-cl 或 gnullvm 工具链
- **影响范围**: neo-crypto, neo-native-contracts, neo-core (所有使用 BLS 签名的模块)
- **严重程度**: 🔴 **CRITICAL** (阻塞完整构建)

#### 错误 2: neo-wallets 依赖缺失 (60+ 编译错误)
```
error[E0433]: cannot find module or crate `scrypt` in this scope
error[E0433]: cannot find module or crate `base64` in this scope
error[E0433]: cannot find module or crate `hex` in this scope
error[E0433]: cannot find module or crate `subtle` in this scope
error[E0433]: cannot find module or crate `neo_primitives` in this scope
error[E0433]: cannot find module or crate `serde_json` in this scope
error: cannot find attribute `serde` in this scope
```

**文件**: `neo-wallets/Cargo.toml` (第16-22行)

**当前内容**:
```toml
[dependencies]
bip39 = { workspace = true }
zeroize = { workspace = true }
```

**缺失依赖**:
- `scrypt` (nep2.rs 加密/解密)
- `base64` (nep2.rs 编码)
- `hex` (key_pair.rs, nep6.rs 十六进制编解码)
- `subtle` (key_pair.rs 常数时间比较)
- `neo_primitives` (account.rs Witness trait 实现)
- `serde_json` (nep6.rs JSON 序列化)
- `serde` (nep6.rs derive 宏)

**严重程度**: 🔴 **CRITICAL** (neo-wallets 完全无法编译)

---

### 1.2 cargo check --workspace --all-targets --all-features

**命令**:
```powershell
rustup run 1.99.1-x86_64-pc-windows-gnullvm cargo check --workspace --exclude neo-wallets --all-targets --all-features
```

**结果**: ❌ **FAIL** (ExitCode: 1)

**关键错误**:

#### 错误 3: neo-primitives 使用不稳定特性
```
error[E0658]: use of unstable library feature `assert_matches`
  --> neo-primitives\src\storage.rs:291:9
  --> neo-primitives\src\storage.rs:460:9
  --> neo-primitives\src\storage.rs:463:9
  --> neo-primitives\src\storage.rs:466:9
  --> neo-primitives\src\verification.rs:682:9
```

**根因**: 测试代码使用 `std::assert_matches!` 宏，但该宏在 Rust 1.99.1 中仍为不稳定特性，需要 `#![feature(assert_matches)]` 或替换为稳定替代方案 `assert!(matches!(...))`

**严重程度**: 🟠 **HIGH** (阻塞测试编译)

#### 错误 4: neo-crypto 使用不稳定特性
```
error[E0658]: use of unstable library feature `assert_matches`
  --> neo-crypto\src\bip32.rs:130:9
  --> neo-crypto\src\bip32.rs:135:9
```

**严重程度**: 🟠 **HIGH** (阻塞测试编译)

#### 错误 5: neo-core 44个编译错误

**5.1 未解析导入**:
```
error[E0432]: unresolved import `crate::state_store`
error[E0432]: unresolved import `crate::persistence::write_batch_buffer::WriteBatch`
error[E0432]: unresolved import `crate::state_service::AccountPrefetchCache`
error[E0432]: unresolved import `crate::state_service::metrics_ops`
```

**5.2 缺失模块**:
```
error[E0433]: cannot find module or crate `prefetch_worker` in this scope
  --> neo-core\src\state_service\pipeline_integration_example.rs:196:56
  --> neo-core\src\state_service\pipeline_integration_example.rs:211:50
  --> neo-core\src\state_service\pipeline_integration_example.rs:222:23
  --> neo-core\src\state_service\pipeline_integration_example.rs:290:19
```

**5.3 函数签名不匹配**:
```
error[E0061]: this function takes 8 arguments but 7 arguments were supplied
  --> neo-core\src\smart_contract\helper.rs:408:26
  --> neo-core\src\wallets\asset_descriptor.rs:77:26
  --> neo-core\src\wallets\helper.rs:199:34
  --> neo-core\src\wallets\helper.rs:303:26
  --> neo-core\src\neo_system\persistence.rs:296:37
  --> neo-core\src\tokens_tracker\trackers\nep_11\nep11_tracker.rs:167:30
  --> neo-core\src\tokens_tracker\trackers\nep_17\nep17_tracker.rs:156:30
```

**根因**: `ApplicationEngine::new()` 签名已更改为需要8个参数（新增 `native_services: EngineNativeServices`），但所有调用点仍传递7个参数

**5.4 NativeRegistry API 不匹配**:
```
error[E0599]: no function or associated item named `new` found for struct `NativeRegistry`
  --> neo-core\src\smart_contract\helper.rs:454:51
```

**根因**: `NativeRegistry::new()` 不存在，应使用 `NativeRegistry::empty()`

**5.5 类型不匹配**:
```
error[E0308]: mismatched types
  --> neo-core\src\smart_contract\application_engine\state.rs:370:89
  --> neo-core\src\state_service\prefetch_background.rs:115:44
  --> neo-core\src\state_service\prefetch_worker.rs:189:16
```

**5.6 使用已移动值**:
```
error[E0382]: use of moved value: `config`
  --> neo-core\src\state_service\pipeline_integration_example.rs:121:30
```

**5.7 缺失字段**:
```
error[E0609]: no field `prefetch_cache` on type `application_engine::ApplicationEngine`
```

**5.8 解引用错误**:
```
error[E0614]: type `Result<RwLockWriteGuard<'_, u32>, PoisonError<...>>` cannot be dereferenced
  --> neo-core\src\state_service\pipeline_integration_example.rs:190:9
  --> neo-core\src\state_service\pipeline_integration_example.rs:268:29
```

**严重程度**: 🔴 **CRITICAL** (neo-core 完全无法编译)

#### 错误 6: neo-state-service 未解析导入
```
error[E0432]: unresolved import `crate::state_store`
```

**严重程度**: 🟠 **HIGH**

---

## 2. 依赖 DAG 与源权威验证

### 2.1 循环依赖检查

**命令**: 
```powershell
rustup run 1.99.1 cargo tree --workspace --all-features
```

**结果**: ⚠️ **无法完成** (编译失败阻塞依赖树分析)

### 2.2 neo-core 残留模块检查

**发现**: neo-core/src/lib.rs 仍包含以下应迁移模块：

```rust
pub mod smart_contract;      // 应迁移至 neo-smart-contract
pub mod ledger;              // 应迁移至 neo-ledger
pub mod network;             // 应迁移至 neo-p2p
pub mod persistence;         // 应迁移至 neo-storage
pub mod wallets;             // 应迁移至 neo-wallets
pub mod state_service;       // 应迁移至 neo-state-service
pub mod signing;             // 应迁移至 neo-signing
pub mod neo_system;          // 应迁移至 neo-system (runtime feature)
pub mod oracle_service;      // 应迁移至 neo-oracle (runtime feature)
pub mod tokens_tracker;      // 应迁移至独立crate (runtime feature)
```

**证据**: neo-core/src/lib.rs 第201-289行

**严重程度**: 🔴 **CRITICAL** (物理迁移未完成)

### 2.3 旧路径引用检查

**命令**:
```powershell
grep -r "use neo_core::" --include="*.rs"
```

**发现**: 25+ 处仍引用 neo-core 的旧路径：

```rust
// neo-mempool/src/memory_pool/tests.rs
use neo_core::WitnessScope;
use neo_core::ledger::TransactionVerificationContext;
use neo_core::network::p2p::helper::get_sign_data_vec;
use neo_core::network::p2p::payloads::block::Block;
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::BinarySerializer;
use neo_core::smart_contract::native::AccountState;

// neo-mempool/src/memory_pool/mod.rs
use neo_core::hardfork::Hardfork;
use neo_core::ledger::TransactionVerificationContext;
use neo_core::network::p2p::payloads::transaction_attribute::TransactionAttribute;
use neo_core::network::p2p::payloads::{Transaction, conflicts::Conflicts};
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
```

**根因**: 业务模块仍依赖 neo-core 而非迁移后的权威 crate

**严重程度**: 🟠 **HIGH** (源权威未建立)

---

## 3. 引擎反向接口验证

### 3.1 ApplicationEngine::new() 签名变更

**预期**: 任务 #88 建立引擎原生注册与状态读取反向接口

**实际**: 
- `ApplicationEngine::new()` 签名已更改为需要8个参数（新增 `native_services: EngineNativeServices`）
- 但所有调用点（7+处）仍传递7个参数
- 导致编译失败

**受影响文件**:
- neo-core/src/smart_contract/helper.rs:408
- neo-core/src/wallets/asset_descriptor.rs:77
- neo-core/src/wallets/helper.rs:199, 303
- neo-core/src/neo_system/persistence.rs:296
- neo-core/src/tokens_tracker/trackers/nep_11/nep11_tracker.rs:167
- neo-core/src/tokens_tracker/trackers/nep_17/nep17_tracker.rs:156

**严重程度**: 🔴 **CRITICAL** (引擎接口未正确集成)

### 3.2 NativeRegistry API

**预期**: 提供 `NativeRegistry::new()` 构造函数

**实际**: 
- `NativeRegistry::new()` 不存在
- 仅提供 `NativeRegistry::empty()`
- 调用点使用错误的 API

**文件**: neo-core/src/smart_contract/helper.rs:454

**严重程度**: 🟠 **HIGH**

### 3.3 缺失模块

**预期**: prefetch_worker 模块应存在于 neo-core/src/state_service/

**实际**: 
- 文件 `neo-core/src/state_service/prefetch_worker.rs` 存在
- 但未在 lib.rs 或 mod.rs 中声明
- 导致4处引用失败

**严重程度**: 🟠 **HIGH**

---

## 4. 协议数据物理迁移正确性验证

### 4.1 迁移状态

**发现**: 
- neo-native-contracts 包含从 neo-core 迁移的注释（25+处）
- 但 neo-core 仍保留原始模块
- 未验证编码/解码与哈希计算语义一致性（编译失败阻塞测试执行）

**严重程度**: ⚠️ **无法验证** (编译失败)

---

## 5. 测试与格式验证

### 5.1 cargo test --workspace --all-features

**命令**:
```powershell
rustup run 1.99.1 cargo test --workspace --all-features
```

**结果**: ❌ **无法执行** (编译失败阻塞测试)

### 5.2 cargo fmt --all --check

**命令**:
```powershell
rustup run 1.99.1 cargo fmt --all --check
```

**结果**: ❌ **FAIL** (ExitCode: 1)

**未格式化文件**:

1. **neo-system/src/adapters/dispatch_adapter.rs**
   - 第101行: 函数参数换行不一致
   - 第183行: 函数参数换行不一致
   - 第228行: 闭包格式
   - 第237行: 断言格式

2. **neo-wallets/src/lib.rs**
   - 模块声明顺序不符合 rustfmt 规范

3. **neo-wallets/src/nep2.rs**
   - 第71行: 函数调用参数换行

4. **neo-wallets/src/nep6.rs**
   - 第183行: 函数参数换行
   - 第225行: 函数调用参数换行
   - 第301行: match 分支格式
   - 第317行: 函数调用参数换行
   - 第332行: 链式调用格式
   - 第377行: 函数调用参数换行
   - 第515行: 链式调用格式
   - 第537行: 链式调用格式

5. **neo-wallets/src/wallet.rs**
   - 换行符样式错误 (CRLF vs LF)

6. **neo-wallets/tests/wallet_bip39_tests.rs**
   - 第198行: 多余空行

**严重程度**: 🟡 **MEDIUM** (格式不合规)

---

## 6. 缺陷汇总与修复建议

### 🔴 CRITICAL (阻塞验收)

| ID | 缺陷 | 影响范围 | 修复归属 | 建议修复 |
|----|------|---------|---------|---------|
| C1 | blst 汇编编译失败 (MSVC) | neo-crypto, neo-native-contracts, neo-core | 基础设施/构建系统 | 使用 gnullvm 工具链或配置 CC=clang-cl |
| C2 | neo-wallets Cargo.toml 依赖缺失 | neo-wallets (完全无法编译) | neo-wallets/Cargo.toml | 添加 scrypt, base64, hex, subtle, neo_primitives, serde_json, serde 依赖 |
| C3 | neo-core 44个编译错误 | neo-core (完全无法编译) | neo-core 各模块 | 修复未解析导入、函数签名、类型不匹配等 |
| C4 | neo-core 物理迁移未完成 | 整个工作区架构 | neo-core 拆解工程 | 完成 smart_contract, ledger, network, persistence, wallets, state_service 等模块迁移 |
| C5 | ApplicationEngine::new() 调用点未更新 | neo-core 7+处调用 | neo-core 各调用模块 | 添加第8个参数 native_services |

### 🟠 HIGH (严重影响)

| ID | 缺陷 | 影响范围 | 修复归属 | 建议修复 |
|----|------|---------|---------|---------|
| H1 | neo-primitives 使用不稳定 assert_matches | neo-primitives 测试 | neo-primitives/src/storage.rs, verification.rs | 替换为 assert!(matches!(...)) 或添加 #![feature(assert_matches)] |
| H2 | neo-crypto 使用不稳定 assert_matches | neo-crypto 测试 | neo-crypto/src/bip32.rs | 替换为 assert!(matches!(...)) 或添加 #![feature(assert_matches)] |
| H3 | neo-state-service 未解析导入 | neo-state-service | neo-state-service/src/lib.rs | 检查 state_store 模块是否存在或已迁移 |
| H4 | NativeRegistry::new() API 不匹配 | neo-core helper.rs | neo-core/src/smart_contract/helper.rs:454 | 改为 NativeRegistry::empty() |
| H5 | prefetch_worker 模块未声明 | neo-core state_service | neo-core/src/state_service/mod.rs | 添加 mod prefetch_worker; |
| H6 | 业务模块仍引用 neo-core 旧路径 | neo-mempool 等 | 各业务模块 | 更新为迁移后的权威 crate 引用 |

### 🟡 MEDIUM (需要修复)

| ID | 缺陷 | 影响范围 | 修复归属 | 建议修复 |
|----|------|---------|---------|---------|
| M1 | 格式不合规 | neo-system, neo-wallets | 各文件 | 运行 cargo fmt --all |
| M2 | 换行符样式错误 | neo-wallets/src/wallet.rs | neo-wallets | 统一使用 LF |

---

## 7. 验收标准判定

| 验收项 | 标准 | 实际结果 | 判定 |
|--------|------|---------|------|
| 可编译 | 全工作区编译通过 | 44+ 编译错误 | ❌ **FAIL** |
| 无残留旧引用 | 业务模块引用迁移后权威 crate | 25+ 处仍引用 neo-core 旧路径 | ❌ **FAIL** |
| 语义一致 | 编码/解码与哈希计算一致 | 无法验证（编译失败） | ⚠️ **BLOCKED** |
| 格式合规 | cargo fmt --check 通过 | 多处格式错误 | ❌ **FAIL** |
| 测试通过 | cargo test 通过 | 无法执行（编译失败） | ⚠️ **BLOCKED** |

---

## 8. 结论

**物理迁移与引擎接口未达到验收标准**。

主要问题：
1. **neo-wallets 完全无法编译** - Cargo.toml 依赖严重缺失
2. **neo-core 完全无法编译** - 44个编译错误，包括未解析导入、函数签名不匹配、类型错误等
3. **物理迁移未完成** - neo-core 仍包含大量应迁移模块
4. **引擎接口未正确集成** - ApplicationEngine::new() 调用点未更新
5. **源权威未建立** - 业务模块仍引用 neo-core 旧路径

**建议**:
- 立即修复 neo-wallets Cargo.toml 依赖缺失（C2）
- 修复 neo-core 编译错误（C3, C5）
- 完成物理迁移（C4）
- 更新业务模块引用（H6）
- 修复不稳定特性使用（H1, H2）
- 运行 cargo fmt 修复格式问题（M1）

**后续任务分派建议**:
- 任务 A: 修复 neo-wallets 依赖 (归属: neo-wallets/Cargo.toml)
- 任务 B: 修复 neo-core 编译错误 (归属: neo-core 各模块)
- 任务 C: 完成物理迁移 (归属: neo-core 拆解工程)
- 任务 D: 更新引擎接口调用点 (归属: neo-core 各调用模块)
- 任务 E: 修复格式问题 (归属: 各文件)

---

**报告生成时间**: 2026-11-05  
**验证工具**: rustup 1.99.1, cargo, rustfmt  
**验证方法**: 真实编译、检查、格式验证，非元数据绿灯
