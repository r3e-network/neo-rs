# Neo RS — Coq 形式化覆盖与状态（诚实报告） 2026-09-18

> 判定标准（权威）：`formal/ci/check_coq.py --all`。
> 对每个 `formal/coqlib/*.v` 在**隔离临时目录**中独立执行 `coqc -Q coqlib CoqLib`，仅当编译退出 0 时才运行 `coqchk -silent`。编译/内核任一失败即该模型**不通过**。这不复用项目内任何旧 `.vo`。
> 工具链：Coq 8.18.0 / OCaml 4.14.1（WSL Ubuntu）。源码 SHA-256 见 `reports/formal/coq-baseline-now/summary.json`。

## 1. 全量结论

共 34 个 Coq 模型：**25 通过（coqc 0 + coqchk 0） / 9 未通过**。

一个模型"coqc=0 且 coqchk=0 且无 assumed axioms"才是"已验证"。其余模型**不是**机器可验证证明。

### 已通过（25）
| 文件 | 说明 |
|---|---|
| `rw_set_minimal.v` | 并行执行 RwSet 冲突检测（最小集，内核无公理） |
| `mpt_cache_cleanup_refinement.v` | MRU-first LRU 抽象表（**不**主张对 Arc/lru-crate/Rust 的精化） |
| `example_proof.v` | 教学样例 |
| `gas_price_refinement.v` | 系统调用 gas 非负（对抽象标题，见下） |
| `uint_refinement.v` | 无符号整数长度不变式 |
| `wire_format_refinement.v` | wire 长度/前缀结构 |
| `varint_roundtrip_refinement.v` | varint 1/3/5 字节 roundtrip（**9 字节 2^64 情形未证**） |
| `varint_encoding_refinement.v` | **诚实重写**：编码长度分档(1/3/5/9)、小数 roundtrip、输出字节<256、前缀校验；见 §2c |
| `block_validation_refinement.v` | **诚实重写**，见 §2 |
| `tx_attribute_enum_refinement.v` | **诚实重写**，见 §2a |
| `opcode_price_full_256.v` | **生成器从 Rust 权威表派生**，见 §2d |
| `opcode_price_refinement.v` | **生成器从 Rust 权威表派生**，见 §2d |
| `smart_contract_validation_refinement.v` | **诚实重写**，见 §2e |
| `merkle_tree_refinement.v` | **诚实重写**，见 §2f |
| `block_transaction_roundtrip_refinement.v` | Track5 重建（roundtrip/序列化） |
| `block_validation_roundtrip_refinement.v` | Track5 重建 |
| `change_view_message_refinement.v` | Track5 重建（dBFT 消息） |
| `commit_message_refinement.v` | Track5 重建 |
| `commit_response_refinement.v` | Track5 重建 |
| `consensus_roundtrip_refinement.v` | Track5 重建 |
| `oracle_response_refinement.v` | Track5 重建 |
| `prepare_request_refinement.v` | Track5 重建 |
| `prepare_response_refinement.v` | Track5 重建 |
| `serialization_roundtrip_refinement.v` | Track5 重建 |
| `viewchange_message_refinement.v` | Track5 重建 |

> 说明：`serialization_roundtrip_refinement.v` 早前在子代理独立脚本下通过但权威门禁红，后由 Track5 重建，现已在权威门禁下 `compile 0 + kernel 0`，计入上表。

### 未通过（9）— 全部带 `Admitted`/`Axiom` markers，需诚实重建
| 文件 | markers | 首因 / 审计要点 |
|---|---|---|
| `abi_encoding_refinement.v` | 5 | decode 全返回常量；文件头声称 decode(encode v)=Some v 与实现矛盾（反例 v=ByteString [7]） |
| `crypto_hash_refinement.v` | 7 | 词法错误；输出尺寸引理 + `Admitted`；假"碰撞不可行" |
| `mempool_validation_refinement.v` | 9 | `Nat.Nat`/`Map.*` 幽灵；`mempool_never_exceeds_capacity` 空真 |
| `multisig_depth_refinement.v` | 5 | `Functions` 幽灵；`compare_bytes := true` 占位；`is_sorted := True` |
| `multisig_validation_refinement.v` | 4 | `account_binding` 前提为 `hash=[]`；`is_sorted := True` |
| `parallel_execution_safety_refinement.v` | 5 | 游离 `*)`；4 引理全 `Admitted`；类型错误 `nil =? keys` |
| `state_root_refinement.v` | 6 | `FMap` 幽灵、`[||]`、`bytes` 未定义、`Fixpoint` 内 `admit` |
| `witness_script_hash_refinement.v` | 9 | `Crypto.Hash[.SHA1/HMAC]` 幽灵；redeem=35 字节与 Rust `helper.rs` 40 字节 N3 不符 |
| `witness_validation_refinement.v` | 12 | `Crypto.Hash.SHA1` 幽灵；redeem 35 vs 40；"无欠签名"前提自带签名数≥阈值 ⇒ 空真 |

## 2. 本轮新增可信成果

### 2.1 `block_validation_refinement.v` 诚实重写并验证（PASS）
原文件是审计确认的**伪模型**：非标准命令（`Constraint`、无体 `Constant`、`()` 参数）、Unicode/偏法（`refl*exity`）、陈旧 stdlib 名（`eq_nat`/`lt_nat`/`andb_and`）、无配对 `End test_examples`、以及**假** `valid_block_satisfies_constraints`（primary_index=300、validator_count=257 使析取两支皆假）。

重写后（源码 SHA `8c4b966f`）：Coq 8.18 `coqc=0`，`coqchk=0`。变更要点：
- 删除全部假定理、`admit`、`Axiom` 假命题；哈希/序列化作为**显式 Section 参数/契约**（Hyypothesis `serialize_unsigned_projection`），并在文档中明示"不主张对 SHA-256/Rust 的精化"。
- merkle root 改为**有穷递归** `merkle_with_fuel`（避免非结构终止），单元素/双元素/空情形可证；`merkle_root_unique` 成立。
- 结构谓词加入显式 `validator_count` 前提，`valid_block_satisfies_constraints` 变为诚实的真定理。
- `size_invariant` 公理与 `Property … length (nat)` 类型错误移除。

### 2.2 命名空间统一（Coq 8.18 兼容）
将 24 个文件的 `From Stdlib …` 改为 `From Coq …`（`From Stdlib` 在 8.18 无效）；`Init.Nat` 等路径核对。这一层 24 个文件**第一错误**大多是这些幽灵导入，但**“修完 import 即通过”不成立**——每个文件都在其后有上节所列更深的假命题/伪证明阻断。导入修复使首错前移，未制造任何“假装通过”。

### 2a. `tx_attribute_enum_refinement.v` 诚实重写并验证（PASS）
原文件概念"极低复杂度"（5 变体 + 字节双射）但有真 bug：`to_byte_injective` 手工 25 分支顺序错位，`unknown_bytes_return_none`/`as_str_nonempty` 用 8.18 已移除的 `omega`，`lemma_allows_multiple_conflicts` 缺 `Lemma` 关键字，`valid_bytes : Set nat` 非法。
重写后（源码 SHA `7795e5f0`）coqc=0 + coqchk=0，全自定义、无公理/admit。内容：`TxAttrType` 五个变体、`to_byte`/`from_byte`（用 `Nat.eqb` 守卫）、`as_str`、`allows_multiple`；证明 `to_byte_injective`、`from_byte_to_byte_inverse`（roundtrip）、`unknown_bytes_return_none`（对不在码集的 nat 返回 None，改用 `Nat.eqb_eq` 转换，避免 `omega`）、`as_str_nonempty`、`allows_multiple_conflicts`，以及聚合的 `refinement_theorem`。它是**抽象枚举 + wire 码模型**，不是对完整 Rust 属性处理的精化。

### 2c. `varint_encoding_refinement.v` 诚实重写并验证（PASS）
原文件后半段是 `Admitted`/`admit` 伪证明：对未特化符号 `value` 用 `simpl (write_var_int value)`、引用未定义 `head`、`prefix_of_encoding` 引用未定义项、`norm_num`/`omega`/`Nat.repr` 等 8.18 不可用或类型错误写法，且 `omega` 已移除。
重写后（源码 SHA `b6d51532`）：coqc=0 + coqchk=0，无公理/admit。内容：`encoded_len`/`write_var_int`/`read_var_int_prefix`（**只解码到 u32/5 字节**，u64 返回 None——README 明示）；证明各范围长度 `encoded_len_small/u16/u32/u64` = 1/3/5/9、`write_small_length`、`encode_small_then_decode`（小数 roundtrip）、`small_encode_prefix`、`small_encode_byte_range`（输出字节<256）。
**关键技术**：`MAX_U16`/`MAX_U32` 定义为**算术式**（`256*256-1`、`256*256*256*256-1`）而非 `nat` 字面量——Coq 把 >253 的 nat 字面量解析为 `Init.Nat.of_num_uint ...`，`lia` 无法线性化、`vm_compute` 在 `m<m` 相等大数上栈溢出。用算术式后 `lia` 可直接处理矛盾分支。

### 2d. `opcode_price_full_256.v` / `opcode_price_refinement.v` 生成器派生（PASS）
原两文件对 `Z` 做 256 分支逐 `match`——`Z` 域中 `8` 等不是合法 pattern（编译错 `[eqn] expected after '|'`），且 `opcode_price_refinement` 含**手写错误值**（如 `opcode_price_all 65 = 2`、`70 = 2`，而 Rust 表 65=0、70=0）。
**方法**：新增生成器 `scripts/gen-opcode-coq.py`，用正则从 Rust 权威源 `neo-core/.../op_code_prices.rs` 的 `OPCODE_PRICE_TABLE: [i64; 256]` **精确提取 256 个值**（抽样校验 0→1,12→8,33→1,65→0,197→512,200→8192 全对），生成自包含 Coq 模型：表表示为 `list nat` + `nth`（`opcode_price_all op := nth op price_table 0`，越界→0），避免符号 `if` 链归约难题。
证明：`all_opcodes_non_negative : forall op, opcode_price_all op >= 0`、`opcode_table_size : length price_table = 256`、以及 **256 个 `Example price_i : opcode_price_all i = v_i`**（逐值 machine-checked）。两文件内容相同（同为权威表拆分，各自独立编译）。
**价值**：Coq 表与 Rust 表逐字一致，全部 256 个值机检，无手抄错误、无公理。**边界**：Rust 表本身标注 TODO(M-17) 需与 C# 交叉校验；本模型只证明与 **Rust 表**一致，不宣称与 C# 参考一致。

### 2e. `smart_contract_validation_refinement.v` 诚实重写并验证（PASS）
原文件是审计确认的假命题模型：`multisig_valid_param_range` 的 iff 为假（`valid_multisig_params 0 5 = True` 但 RHS `(1<=0 ∨ …)=False`）；用不存在的 `norm_num`（mathcomp）、`Lt.lt_trans`；`Open Scope Z_scope` 泄漏使 nat 的 `>=`/`<=` 变成 Z 符号（line 36 类型错）；`consume_gas` 用截断减法。
重写后（源码 SHA `a3b929d9`）coqc=0 + coqchk=0，无公理/admit。内容：`bytes`/`valid_byte`/`valid_script`、`MAX_SCRIPT_SIZE := 1024*1024`（**算术式**，避免 `of_num_uint` 大字面量）、`SmartContract`/`InvocationContext`/`consume_gas`（守卫改为 `consumed+amount<=max`，避开截断减法）、`valid_multisig_params := 1<=m<=n<=1024`。
证明：`script_size_bound`、`consume_gas_preserves_validity`、`contract_hash_congruence`、**修正后的** `multisig_valid_param_range`（真 iff）、`empty_script_valid_and_bounded`，及小预算 Examples。**关键技巧**：`0 < 1024*1024` 用 `Nat.mul_pos_pos` 分解（避免 `vm_compute` 展开乘积栈溢出）；`3 <= 1024` 用 `apply Nat.leb_le; vm_compute; reflexivity`；record 投影 `ctx.(field)` 需显式语法；`lia` 前需 `simpl` 归约 record 投影。

### 2f. `merkle_tree_refinement.v` 诚实重写并验证（PASS）
原文件是审计确认的伪代码（注释里含 "this is wrong... let me trace carefully"），含幽灵 `Function`/`Coq.Strings.ByteString` 导入、Unicode 箭头 `→`/`∘`、以及 `list_len` 重定义 `length`、`Notation "x == y"`。
重写后（源码 SHA `b0ee42f2`）coqc=0 + coqchk=0，无公理/admit。模型**忠实对应 `neo-crypto/src/merkle_tree.rs::compute_root`**：每层 `merkle_level` 相邻配对、末元素奇数时与自身配对（`[x] -> [hash x x]`）；`hash_pair l r := sha256(l++r)`（对应 Rust 64 字节拼接）。`sha256` 为显式 Section 参数（抽象，不主张抗碰撞）。
证明：`compute_root_empty/single/two/three`（含三元素 `hash_pair(hash_pair h1 h2)(hash_pair h3 h3)`）、`compute_root_unique`、`compute_root_deterministic`。修正了原 `merkle_level_length` 引理（单元素被复制，长度非 div2，故删去该错误引理）。

### 2.3 形式化 CI 改为 fail-closed（本地证据）
`formal-verification.yml` 与 `formal/ci/run_tlc_ci.sh`、`test_run_tlc_ci.sh` 重写：
- Coq `--all` 设为**必需且失败即 red**；删除吞错的 `|| echo`、假的 Krilla 作业、异常 workflow 结构。
- TLC 用现存三组有界配置 + `NoFinalization` 非空确理由；TLA+ 工具 v1.7.4 经 SHA-256 校验。
- 本地证据：`formal/ci/evidence-local-tlc/`（四配置 PASS，含非空确 witness exit 12）、`formal/ci/evidence-local-coq/`（全量 red 属预期）。actionlint + 16 场景驱动回归通过。

## 3. 对既往失实声明的更正（PHASE/AUDIT）
以下在过去文档中被写成“已完成/已验证”，与机器检查不符，必须更正：
- “All Coq differential testing complete / Production-ready”、“8 security-critical proofs”、“Coq block roundtrip `deserialize(serialize x)=Some x` 已证”等——均为**未通过或空真/公理**，本报告全量门禁即证据。
- “486 测试”多数为 Rust 侧测试/降尺重建，不代表 Coq 证明通过。
- `merkle_root_associative` 左右同项、`view_change_trigger_valid` 恒真等“关键引理”为零内容。

## 4. 边界与免责
- Coq 模型是**抽象模型/合同**，不是对 Rust 可执行文件的精化证明。哈希等密码原语为显式参数，不主张抗碰撞。
- `varint` 9 字节、`serialization` 全量、`block_transaction` 真 roundtrip 等，需用 wire 级别模型重建后方可声称。
- TLC 覆盖有算子高度的 party 抽象，非完整 dBFT。

## 5. 下一步（按优先级，需对应活动）
1. 重建 wire/序列化与 roundtrip 模型（`block_transaction`、`serialization`、`varint` 9 字节）：以 `neo-io`/`serializable` 为 spec，保证 encode/decode 双向可证。
2. 逐值核对 Rust `OPCODE_PRICE_TABLE[256]`，重建 `opcode_price_refinement`/`opcode_price_full_256` 为可穷尽且非负可证的 `nat→Z` 或表索引模型。
3. 共识消息模型（`consensus/commit/change_view/viewchange/prepare_*`）从 dBFT 真结构重建，去掉 `Maybe`/恒真重言。
4. witness/redeem：与 `helper.rs`（40 字节 N3）对齐后再断言格式；mempool/multisig 的排序/绑定需真实结构。
5. ABI/state_root/merkle 按 Rust 真实现重建。
6. 主线：将"已验证"集合接入 CI 预定子集；每新增一个通过即登记 SHA 与审计证据。