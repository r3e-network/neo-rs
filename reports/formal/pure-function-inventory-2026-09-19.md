# Neo-rs — 纯函数形式化验证盘点与分阶段路线

**日期**: 2026-09-19
**范围**: 只读盘点（grep / Read 源核实），未修改任何源文件
**目标**: 建立"扩展到 ~200 个纯函数验证"的**可执行候选清单**（非现在去验证），给出按模块/类型组织、标注复杂度、所属 crate、验证方式与优先级。

---

## 0. 盘点结论（诚实数字）

- **识别候选纯函数总数: ~285**（已逐条对到源码 file:line，见 JSON 附件 `reports/formal/pure-functions.json`）。
- **"~200"目标可达成且已被超过**。超出部分多为同类族函数（枚举校验、位运算、序列化读写器），验证成本低。
- **判断标准**: 纯函数 = 无 IO、无全局可变状态（不含 `static mut`/`RwLock`/`Mutex` 全局缓存）、输入仅来自自身参数（含 `&self`）。密码学原语（SHA256/BLAKE2b/ECC）视为**纯但黑盒**——Coq 只能验证包装/长度/组合逻辑，不能证抗碰撞。
- **被排除（不纳入）**: 网络/p2p 收发、存储 RocksDB、异步/`async fn`、`generate_private_key`（依赖 RNG/系统熵）、数据库缓存访问（`mpt_trie/cache` 全局 `GLOBAL_NODE_CACHE`）、共识 `save/load`（IO）等。

### 当前验证状态参照
权威门禁（`formal/ci/check_coq.py --all`）**: 25 / 34 个 Coq 模型通过**。已通过的与纯函数相关的模型：`varint_encoding_refinement`、`varint_roundtrip_refinement`、`uint_refinement`、`wire_format_refinement`、`merkle_tree_refinement`、`opcode_price_full_256`、`opcode_price_refinement`、`smart_contract_validation_refinement`、`tx_attribute_enum_refinement`、`block_validation_refinement` 及 12 个共识消息 roundtrip 模型。
**未通过、需重建**：`abi_encoding_refinement`（decode 恒返回常量，与 `fast_codec.rs` 矛盾）、`crypto_hash_refinement`（词法错误 + 假碰撞）、`state_root_refinement`、`witness_*`（redeem 35 vs 40 字节不符）、`mempool_validation_refinement`、`multisig_*`、`parallel_execution_safety_refinement`。

---

## 1. 按 crate 分布

| crate | 候选数 | 主要热点 |
|---|---|---|
| neo-io | ~45 | varint/varbytes、serializable helper、BinaryWriter/MemoryReader LE 编解码、LZ4 边界校验 |
| neo-crypto | ~55 | hash 族、encoding（base58/64/hex）、merkle、murmur、bloom、ECC/签名校验 |
| neo-vm | ~100 | abi/stack_value 编解码、fast_codec、semantics（arithmetic/collections/comparison/conversion/splice/numeric）、script/script_builder、binary_serializer、syscall gas |
| neo-primitives | ~45 | UInt160/256（宏生成）、uint_hex、base58_check、big_decimal、枚举校验 |
| neo-core | ~15 | validation（block 纯校验）、witness、smart_contract/helper redeem、op_code_prices |
| neo-consensus | ~25 | context（f/m/commits 阈值）、messages 纯校验/序列化 |
| **合计** | **~285** | |

> 注：UInt 类型实际定义在 `neo-primitives`（`uint_type!` 宏），`neo-core` 无独立 UInt。ABI 定义在 `neo-vm`。`neo-primitives` 虽不在任务列出的核心库清单里，但它是 UInt/枚举/地址编码的实际归属，且已有 `proptest` 与 Coq `uint_refinement`，故纳入。

---

## 2. Phase A（~55 个高价值纯函数）——编码 / 哈希 / 序列化 / gas

> 优先：已被 Coq 门禁或差异测试部分覆盖、或已知有漏洞缺口的热点。每批可独立交付。

### A1. neo-io varint / varbytes（`neo-io/src/var_int.rs`）
| 函数 | 行 | 签名 | 复杂度 | 现有覆盖 | 建议方式 |
|---|---|---|---|---|---|
| `read_var_int_prefix` | 16 | `(&[u8]) -> Option<(u64,usize)>` | 中 | Coq(varint 模型，u64 未证) | Coq roundtrip + 差异 |
| `write_var_int` | 51 | `(u64,&mut Vec<u8>)` | 中 | 单测 | Coq encode 长度分档 |
| `encoded_len` | 69 | `const fn (u64) -> usize` | 低 | Coq 已证 1/3/5 | 已覆盖 |
| `write_var_bytes` | 82 | `(&[u8],&mut Vec<u8>)` | 低 | 单测 | Rust 属性 |

### A2. serializable helper（`neo-io/src/serializable/helper.rs`）
| 函数 | 行 | 复杂度 | 建议 |
|---|---|---|---|
| `get_var_size` / `get_var_size_usize` / `get_var_size_bytes` / `get_var_size_str` | 9/16/23/30 | 低 | Coq/Rust 属性（长度不变式） |
| `get_var_size_serializable_slice` / `get_var_size_for_slice` | 36/42 | 低 | Rust 属性 |
| `serialize_array` / `serialize_array_with` | 50/59 | 低 | Coq roundtrip |
| `deserialize_array` / `deserialize_array_with` / `deserialize_exact_array` | 75/94/120 | 中 | Coq（越界拒绝） |

### A3. 二进制读写器（`neo-io/src/binary_writer.rs`, `memory_reader.rs`, `extensions/*`）
`BinaryWriter`: `write_u8/16/32/64`、`write_i16/i32/i64`、`write_bool`、`write_bytes`、`write_var_int`、`write_var_bytes`、`write_var_string`、`write_serializable_vec`、`into_bytes`、`len`/`is_empty`（行 50–167）。
`MemoryReader`: `read_boolean`、`read_u8/u16/u32/u64`、`read_i16/i32/i64`、`read_var_int`、`read_fixed_string`、`read_var_string`、`read_bytes`、`remaining`、`position`（行 58–316）。
复杂度低；建议：**Rust proptest 双向 roundtrip + Coq 抽象 wire 模型**（对应未通过的 `serialization` roundtrip 重建目标）。

### A4. 哈希（`neo-crypto/src/hash.rs`）——差异测试已覆盖多数
`sha256`(100) `sha512`(114) `keccak256`(130) `sha3_256`(138) `sha3_512`(146) `ripemd160`(160) `blake2b`(174) `blake2b_512`(179) `blake2b_256`(200) `blake2s`(231) `hash160`(255) `hash256`(278) `hash`(292) `ct_hash_eq`(330) `ct_hash_slice_eq`(346)。
现有: `neo-crypto/tests/property_tests.rs` 有 sha256/512/ripemd160/hash160/hash256/keccak/blake2b/blake2s **proptest 一致性**。未通过 Coq `crypto_hash_refinement`。建议：**差异测试（C# 向量）+ Coq 仅证输出长度/组合（hash160=SHA→RIPEMD、hash256=双重）**。

### A5. 编码（`neo-crypto/src/encoding.rs`）
`Base58::{encode,decode,encode_check,decode_check}`(13–32)、`Base64::{encode,decode,decode_dotnet_semantics,decode_lenient,url_encode_no_pad,url_decode_no_pad_lenient}`(56–104)、`Hex::{encode,decode}`(118–124)、`base58` 模块 4 个包装(135–153)。
现有: proptest roundtrip（base58/base58check/hex）。建议：**差异测试 + Coq 校验和长度规则**；`decode_dotnet_semantics` 是关键（对应 StdLib base64Decode）。

### A6. Merkle（`neo-crypto/src/merkle_tree.rs`）——Coq 模型已通过
`new`(40) `depth`(83) `compute_root`(92) `compute_root_with_tree`(121) `root`(127) `hash_pair`(133) `trim`(141) `to_hash_array`(160) `hash_pair_internal`(183)。Coq `merkle_tree_refinement.v` 已 PASS。建议：**保持，补 Rust 属性（deterministic / 奇数复制语义）**。

### A7. Murmur（`neo-crypto/src/murmur.rs`）
`murmur32`(8) `murmur128`(14)。现有: 固定向量测试。建议：**差异测试（C# 向量）+ Rust 属性**。

### A8. ABI stack value 编解码（`neo-vm/src/abi/`）——**最高优先级（现有 Coq 模型未通过）**
`stack_value.rs`: `encode_integer`(365) `normalize_stack_item_type_tag`(165) `default_value_for_type_tag`(186) `new_array_default_value_for_type_tag`(207) `new_array_default_value_for_neovm_type_tag`(217) `pop_byte_arg`(230) `byte_sequence_bytes`(246) `byte_sequence_len`(255) `stack_value_as_bool/i64/u32/u8/bytes/span_bytes/fixed_bytes/string`(264–334) `stack_value_into_items`(338) `concat_splice_values`(347) `slice_splice_value`(355) `StackValue::{compact_type_tag,to_bool,to_i128,as_bytes,to_byte_string_bytes,convert_to_byte_string_value,convert_to_buffer_value}`(422–521) `StackItemType::{from_byte,to_byte,name}`(110–147)。
`fast_codec.rs`: `encode_stack`(18) `encode_stack_to_slice`(27) `decode_stack`(41) `encode_stack_into`(98) `encode_value_into`(146) `decode_value_depth`(254)。
**要点**: Coq `abi_encoding_refinement.v` 声称 decode(encode v)=Some v 与 `fast_codec.rs` 实现矛盾（反例 ByteString [7]），需**先以 Rust 为 spec 重建**。建议：**Coq roundtrip + 差异测试 + 深度/长度拒绝属性**。

### A9. Opcode / syscall gas（`neo-core/src/smart_contract/application_engine/op_code_prices.rs`, `neo-vm/src/syscalls/static_registry.rs`）
`get_opcode_price`(36)、`OPCODE_PRICE_TABLE[256]`(const)、`compute_syscall_gas_price`(184) `compute_syscall_call_flags`(227) `classify_syscall` `get_gas_cost`(421)。
现有: Coq `opcode_price_full_256`/`opcode_price_refinement`（生成器派生，**PASS**）+ `property_tests.rs::registered_syscall_gas_prices_are_non_negative` + `syscall_gas_prices_match_coq_refinement_model`。
建议：**保持，逐值核对 C#（TODO M-17）**；`compute_syscall_gas_price` 补 Coq 非负 + 差异。

---

## 3. Phase B（~75 个）——VM 语义、UInt/地址、big_decimal、共识消息

> 独立可交付批次；多数为 `StackValue` 上的纯函数，输入只有参数，非常适合 Coq/属性。

### B1. VM 算术（`neo-vm/src/semantics/arithmetic.rs`）
`invert_value`(44) `add_values`(56) `sub_values`(64) `mul_values`(72) `div_values`(80) `modulo_values`(89) `negate_value`(98) `abs_value`(103) `sign_value`(111) `inc_value`(118) `dec_value`(126) `pow_values`(134) `sqrt_value`(149) `modmul_values`(158) `modpow_values`(174) `shl_value`(192) `shr_value`(205) `shl_value_pre_gorgon`(216) `shr_value_pre_gorgon`(231) `bitwise_and/or/xor_values`(245/250/255) `max_values`(300) `min_values`(310) `within_values`(320)。建议：**Coq 算术模型 + Rust 属性**；`pre_gorgon` 为硬分叉差异点（差异测试）。

### B2. 集合（`neo-vm/src/semantics/collections.rs`）
`collection_index_value`(14) `validate_map_key_value`(27) `primitive_key_equal`(43) `map_entry_index`(54) `new_array`(75) `new_array_t`(81) `new_struct`(90) `new_buffer`(96) `new_map`(106) `append`(111) `set_item`(122) `pick_item`(168) `remove`(196) `size`(221) `has_key`(234) `keys`(267) `values`(277) `pack`(289) `unpack`(294) `reverse_items`(306) `clear_items`(317) `pop_item`(332) `pack_struct`(356) `pack_map`(362)。建议：**Coq + 属性**；`set_item/pick_item/remove` 为 key 类型校验热点。

### B3. 比较 / 转换 / 拼接（`semantics/comparison.rs`, `conversion.rs`, `splice.rs`, `numeric.rs`）
`comparison.rs`: `equal_values`(12) `num_equal_values`(23) `less_than_values`(33) `less_or_equal_values`(38) `greater_than_values`(43) `greater_or_equal_values`(48) `bool_and`(82) `bool_or`(88) `nz_value`(93) `boolean_value`(121) `strict_boolean_value`(126) `not_value`(148) `is_null`(154)。
`conversion.rs`: `is_type`(14) `convert_value`(25)。
`splice.rs`: `cat_values`(10) `substr_value`(24) `left_value`(35) `right_value`(48) `memcpy_bytes`(62)。
`numeric.rs`(pub(crate)): `decode_signed_le_bytes_i64`(15) `decode_signed_le_bytes_i128`(50) `decode_signed_le_bytes_bigint`(69) `trim_le_bytes`(92) `trim_le_bytes_slice`(113) `bytes_to_integer`(135) `mod_pow_bigint`(172) `bitwise_signed_bytes`(212)。

### B4. UInt160/256（`neo-primitives/src/uint160.rs`, `uint256.rs`, `macros.rs`）
宏生成（`macros.rs`): `new`(34) `zero`(39) `is_zero`(44) `as_bytes`(49) `to_bytes`(54) `from_bytes`(62) `try_from_span`(84) `from_span`(90) `to_array`(103) `get_span`(120) `parse`(124) `try_parse`(140) `to_hex_string`(151)。
`uint160.rs`: `from_script`(29) `to_address`(43) `from_address`(53) `hash_code`(64) `equals`(79)。`uint256.rs`: `hash_code`(26) `equals`(38)。
现有: `neo-primitives/tests/property_tests.rs` proptest（bytes/hex roundtrip、hash 一致性、equals 对称）+ Coq `uint_refinement`。建议：**保持 + 差异（C# vector）**。

### B5. 地址 / hex / base58check（`neo-primitives/src/uint_hex.rs`, `base58_check.rs`）
`uint_hex.rs`: `parse_reversed_hex`(3) `format_reversed_hex`(19)。
`base58_check.rs`: `encode_check`(52) `decode_check`(57) `encode_address_payload`(66) `decode_address_payload`(74)。
建议：**差异（C#）+ Coq 校验和/版本字节规则**（`witness_script_hash_refinement` 未通过即因 redeem 长度不符，先对齐 40 字节）。

### B6. big_decimal（`neo-primitives/src/big_decimal.rs`）
`new`(77) `sign`(104) `change_decimals`(123) `to_big_integer`(151) `try_parse`(166) `parse`(187)。复杂度中；建议：**Coq 定点数换算 + 属性**。

### B7. 共识消息纯校验 / 序列化（`neo-consensus/src/messages/*`）
`change_view.rs`: `new`(24) `new_view_number`(41) `message_type`(49) `validate`(90)。
`commit.rs`: `new`(23) `message_type`(39) `validate`(50)。
`prepare_request.rs`: `new`(33) `message_type`(57) `deserialize_body`(86) `validate`(206)。
`prepare_response.rs`: `new`(23) `message_type`(39) `deserialize_body`(50) `validate`(73)。
`recovery.rs`: `new`(28/203) `message_type`(44/218) `validate`(324)。
`mod.rs`: `to_message_bytes`(87) `from_message_bytes`(98)。
现有: 12 个 Coq roundtrip 模型 + `consensus_properties.rs`（payload roundtrip、injectivity、wire length invariants）。建议：**保持 + 差异（C#）**。

### B8. 共识 context 阈值（`neo-consensus/src/context/mod.rs`）——纯派生量
`validator_count`(259) `f`(265) `m`(271) `primary_index`(277) `is_primary`(290) `is_backup`(296) `has_enough_prepare_responses`(305) `can_sign_commit`(344) `has_enough_commits`(350) `has_enough_change_views`(375) `count_committed`(686) `count_failed`(701) `more_than_f_nodes_committed_or_lost`(734) `view_changing`(743)。
建议：**Coq 阈值模型 + TLA+ 已覆盖的 commit 阈值交叉验证**。

### B9. 枚举纯校验（`neo-primitives/src/*`）
`witness_scope.rs`: `has_flag`(90) `contains`(100) `combine`(106) `bits`(114) `from_bits`(120) `intersects`(126) `from_byte`(132) `to_byte`(144) `is_valid`(150)。
`transaction_attribute_type.rs`: `allows_multiple`(25)（Coq `tx_attribute_enum_refinement` 已 PASS）。
`contract_parameter_type.rs`: `from_string`(47) `try_from_u8`(53)。`call_flags.rs`、`trigger_type.rs`、`inventory_type.rs`、`oracle_response_code.rs` 等宏生成 `to_byte/from_byte/count/all`。建议：**Coq 枚举双射 + 属性**。

---

## 4. Phase C（~85 个）——ECC/签名、bloom、script、block 校验、helper、其余栈提取器

> 低优先级或高复杂度；批量推进，每类为一批。

### C1. ECC / 签名（`neo-crypto/src/ecc.rs`, `signature.rs`）— 复杂度高
`ecc.rs`: `compressed_size`(74) `uncompressed_size`(83) `is_compressed`(311) `is_valid`(322) `is_on_curve`(393) `is_infinity`(378) `encode_compressed`(274) `encode_point`(335) `decode_compressed_with_curve`(242) `decode_compressed`(254) `decode_secp256r1`(259) `decode_secp256k1`(264) `decode_ed25519`(269) `curve`(280)。
`signature.rs`: `verify_signature_secp256r1`(477) `verify_signature_secp256k1`(483) `verify_signature_with_curve`(489) `verify_signature_bytes`(515) `verify`(Ecdsa 208 / Sm2 308) `recover_public_key`(98) `compress_public_key`(467)。
建议：**差异测试（C#/secp256r1 向量）为主**；ECC 群运算 Coq 成本极高，仅证编码/长度/解析拒绝。

### C2. Bloom filter（`neo-crypto/src/bloom_filter.rs`）
`new`(21) `with_bits`(43) `check`(67) `bit_index`(110) `bloom_seed`(114)。建议：**Rust 属性（插入后必命中）+ 差异**。

### C3. Script 校验 / builder（`neo-vm/src/script.rs`, `script_builder.rs`）
`script.rs`: `validate`(206) `validate_strict`(213) `get_instruction`(236) `get_byte`(277) `get_jump_offset`(353) `hash`(369) `hash_code`(375) `get_jump_target`(388) `get_try_offsets`(406) `get_next_instruction`(424) `len`(313) `is_empty`(326)。
`script_builder.rs`: `emit_push_int`(89) `emit_push_bool`(126) `emit_push_byte_array`(136) `emit_push_string`(142) `emit_push_bytes`(148) `emit_push_bigint`(153) `emit_push_stack_value`(212) `emit_jump`(269) `emit_call`(295) `emit_syscall`(306) `emit_syscall_hash`(318) `to_array`(337)。
建议：**差异（C# 脚本字节）+ Rust 属性**（jump offset 越界、push 最小编码）。

### C4. Block 纯校验（`neo-core/src/validation.rs`）
`validate_block_size_raw`(151) `validate_transaction_count_raw`(184) `validate_timestamp_bounds`(209) `validate_block_version`(381) `validate_primary_index`(398) `validate_merkle_root`(265) `validate_no_duplicate_transactions`(308)。
现有: Coq `block_validation_refinement`（PASS）+ `block_validation_compliance_tests`。建议：**保持 + 差异**。

### C5. Witness / helper（`neo-core/src/witness.rs`, `smart_contract/helper.rs`）
`witness.rs`: `script_hash`(129) `size`(140) `verify_signature`(172) `verify_multi_signature`(191)。
`helper.rs`: `signature_contract_cost`(31) `multi_signature_contract_cost`(38) `is_standard_contract`(66) `is_signature_contract`(71) `is_multi_sig_contract`(87) `to_script_hash`(92) `signature_redeem_script`(97) `try_multi_sig_redeem_script`(116)。
建议：**先对齐 redeem 长度（N3 40 字节）再 Coq**（修复未通过的 `witness_*`）。

### C6. 序列化栈（`neo-vm/src/binary_serializer.rs`, `json_serializer.rs`）
`binary_serializer.rs`: `deserialize_stack_value`(224) `deserialize_stack_value_with_limits`(233) `serialize`(372) `serialize_stack_value`(384) `serialize_with_limits`(393) `serialize_into`(404)。建议：**Coq roundtrip（当前 `serialization` 模型未通过，需重建）**。

### C7. 其余栈提取器 / 杂项
`StackValue` 其余 extractor、`method_token.rs` `size`/`new`(39/100)、`neo-io/src/serializable/primitives.rs` UInt160/256 `size`/`deserialize`(16–44)、`neo-crypto/src/bip32.rs` `add_private_keys_mod_order`(46) `hmac_sha512`(31)、`named_curve_hash.rs` `curve`(25) `hash_algorithm`(34)。

---

## 5. 推荐首批 Phase A 清单（Top 交付批次）

1. **ABI stack_value + fast_codec roundtrip**（修复 Coq `abi_encoding_refinement` 反例，对应 Block 4）— 证据最强缺口。
2. **varint 9 字节 + varbytes 全量 roundtrip**（补 Coq 模型 u64 缺口）— 低复杂度高价值。
3. **hash 族差异向量 + 输出长度 Coq**（sha256/hash160/hash256/blake2b/sha3）— 差异测试已就绪。
4. **base64 `decode_dotnet_semantics` / hex / base58check** — 对应 StdLib syscall。
5. **merkle `compute_root` 保持 + 奇数复制属性**（Coq 已 PASS，补 Rust 属性）。
6. **syscall `compute_syscall_gas_price` + opcode 表逐值核对 C#（M-17）**。
7. **共识消息纯校验 + context f/m/commits 阈值 Coq**（12 个 roundtrip 模型已 PASS，补阈值）。

---

## 6. 可靠性 / 局限说明

- 每个条目均已用 grep/Read 核对真实 `file:line`；不确定项标注"待核"。
- `semantics/numeric.rs`、`stack_shape.rs` 为 `pub(crate)`（非公开 API），但仍为纯函数，纳入时可降低优先。
- ECC 群运算（`ecc.rs`、`signature.rs`）为纯但高复杂度，Coq 直接建模不现实，建议以**差异测试 + 向量**为主；不要在 Coq 上投入。
- `generate_private_key`（RNG/系统熵）、`sign`（依赖内部随机 nonce，见 `signature.rs`）**不是纯函数**，排除。
- 密码原语（SHA256/BLAKE2b/ECC）无法在 Coq 内证明抗碰撞；Coq 只验证包装/长度/组合/roundtrip，模型文档需明示"不主张对 Rust 可执行精化"。
- 机器可读清单见 `reports/formal/pure-functions.json`（字段: name, file, line, crate, signature, complexity, existing_coverage, suggested_method, phase）。
