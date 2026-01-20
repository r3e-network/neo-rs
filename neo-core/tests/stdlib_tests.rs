use neo_core::hardfork::Hardfork;
use neo_core::ledger::block_header::BlockHeader;
use neo_core::ledger::Block;
use neo_core::network::p2p::payloads::{signer::Signer, transaction::Transaction};
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::native::{NativeContract, StdLib};
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::witness::Witness;
use neo_core::{IVerifiable, UInt160, WitnessScope};
use neo_vm::{OpCode, ScriptBuilder, StackItem};
use num_traits::ToPrimitive;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

fn make_engine() -> ApplicationEngine {
    make_engine_with_height(None)
}

fn make_engine_with_settings(settings: ProtocolSettings) -> ApplicationEngine {
    const TEST_GAS_LIMIT: i64 = 400_000_000;
    let mut container = Transaction::new();
    container.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
    container.add_witness(Witness::new());
    let script_container: Arc<dyn IVerifiable> = Arc::new(container);
    ApplicationEngine::new(
        TriggerType::Application,
        Some(script_container),
        Arc::new(DataCache::new(false)),
        None,
        settings,
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine")
}

fn make_engine_with_height(height: Option<u32>) -> ApplicationEngine {
    const TEST_GAS_LIMIT: i64 = 400_000_000;
    let mut container = Transaction::new();
    container.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
    container.add_witness(Witness::new());
    let script_container: Arc<dyn IVerifiable> = Arc::new(container);
    let persisting_block = height.map(|index| {
        let header = BlockHeader {
            index,
            ..Default::default()
        };
        Block::new(header, Vec::new())
    });
    ApplicationEngine::new(
        TriggerType::Application,
        Some(script_container),
        Arc::new(DataCache::new(false)),
        persisting_block,
        Default::default(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine")
}

fn emit_stdlib_call(
    sb: &mut ScriptBuilder,
    stdlib_hash: UInt160,
    method: &str,
    mut args: Vec<StackItem>,
) {
    let arg_count = args.len();
    for arg in args.drain(..).rev() {
        sb.emit_push_stack_item(arg).expect("emit arg");
    }
    sb.emit_push_int(arg_count as i64);
    sb.emit_opcode(OpCode::PACK);
    sb.emit_push_int(CallFlags::ALL.bits() as i64);
    sb.emit_push_string(method);
    sb.emit_push_byte_array(&stdlib_hash.to_bytes());
    sb.emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call syscall");
}

#[test]
fn stdlib_binary_encoding_matches_csharp() {
    let stdlib = StdLib::new();
    let mut sb = ScriptBuilder::new();

    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "base64Encode",
        vec![StackItem::from_byte_string(vec![1, 2, 3, 4])],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "base58Encode",
        vec![StackItem::from_byte_string(vec![1, 2, 3, 4])],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "base64Decode",
        vec![StackItem::from_byte_string(b"A \r Q \t I \n D".to_vec())],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "base58Decode",
        vec![StackItem::from_byte_string(b"2VfUX".to_vec())],
    );
    sb.emit_opcode(OpCode::RET);

    let mut engine = make_engine();
    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.result_stack().len(), 4);
    let decoded_base58 = engine.result_stack().peek(0).unwrap().as_bytes().unwrap();
    assert_eq!(decoded_base58, vec![1, 2, 3, 4]);
    let decoded_base64 = engine.result_stack().peek(1).unwrap().as_bytes().unwrap();
    assert_eq!(decoded_base64, vec![1, 2, 3]);
    let encoded_base58 = engine.result_stack().peek(2).unwrap().as_bytes().unwrap();
    assert_eq!(String::from_utf8(encoded_base58).unwrap(), "2VfUX");
    let encoded_base64 = engine.result_stack().peek(3).unwrap().as_bytes().unwrap();
    assert_eq!(String::from_utf8(encoded_base64).unwrap(), "AQIDBA==");
}

#[test]
fn stdlib_itoa_atoi_parity() {
    let stdlib = StdLib::new();
    let mut sb = ScriptBuilder::new();

    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "itoa",
        vec![StackItem::from_int(1), StackItem::from_int(10)],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "itoa",
        vec![StackItem::from_int(1), StackItem::from_int(16)],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "itoa",
        vec![StackItem::from_int(-1), StackItem::from_int(10)],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "itoa",
        vec![StackItem::from_int(-1), StackItem::from_int(16)],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "itoa",
        vec![StackItem::from_int(1_000_000_000), StackItem::from_int(16)],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "atoi",
        vec![
            StackItem::from_byte_string(b"-1".to_vec()),
            StackItem::from_int(10),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "atoi",
        vec![
            StackItem::from_byte_string(b"+1".to_vec()),
            StackItem::from_int(10),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "atoi",
        vec![
            StackItem::from_byte_string(b"ff".to_vec()),
            StackItem::from_int(16),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "atoi",
        vec![
            StackItem::from_byte_string(b"FF".to_vec()),
            StackItem::from_int(16),
        ],
    );
    sb.emit_opcode(OpCode::RET);

    let mut engine = make_engine();
    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.result_stack().len(), 9);
    let atoi_ff = engine
        .result_stack()
        .peek(0)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(atoi_ff, -1);
    let atoi_ff_upper = engine
        .result_stack()
        .peek(1)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(atoi_ff_upper, -1);
    let atoi_plus = engine
        .result_stack()
        .peek(2)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(atoi_plus, 1);
    let atoi_minus = engine
        .result_stack()
        .peek(3)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(atoi_minus, -1);
    let itoa_big = engine.result_stack().peek(4).unwrap().as_bytes().unwrap();
    assert_eq!(String::from_utf8(itoa_big).unwrap(), "3b9aca00");
    let itoa_hex_neg = engine.result_stack().peek(5).unwrap().as_bytes().unwrap();
    assert_eq!(String::from_utf8(itoa_hex_neg).unwrap(), "f");
    let itoa_neg = engine.result_stack().peek(6).unwrap().as_bytes().unwrap();
    assert_eq!(String::from_utf8(itoa_neg).unwrap(), "-1");
    let itoa_hex = engine.result_stack().peek(7).unwrap().as_bytes().unwrap();
    assert_eq!(String::from_utf8(itoa_hex).unwrap(), "1");
    let itoa_dec = engine.result_stack().peek(8).unwrap().as_bytes().unwrap();
    assert_eq!(String::from_utf8(itoa_dec).unwrap(), "1");
}

#[test]
fn stdlib_atoi_invalid_inputs_fault() {
    let stdlib = StdLib::new();
    let cases = [("a", 10), ("g", 16), ("a", 11)];

    for (value, base) in cases {
        let mut sb = ScriptBuilder::new();
        emit_stdlib_call(
            &mut sb,
            stdlib.hash(),
            "atoi",
            vec![
                StackItem::from_byte_string(value.as_bytes().to_vec()),
                StackItem::from_int(base),
            ],
        );
        sb.emit_opcode(OpCode::RET);

        let mut engine = make_engine();
        engine
            .load_script(sb.to_array(), CallFlags::ALL, None)
            .expect("load script");
        assert!(engine.execute().is_err());
    }
}

#[test]
fn stdlib_memory_compare_parity() {
    let stdlib = StdLib::new();
    let mut sb = ScriptBuilder::new();
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memoryCompare",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"c".to_vec()),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memoryCompare",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"d".to_vec()),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memoryCompare",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"abc".to_vec()),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memoryCompare",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"abcd".to_vec()),
        ],
    );
    sb.emit_opcode(OpCode::RET);

    let mut engine = make_engine();
    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.result_stack().len(), 4);
    let last = engine
        .result_stack()
        .peek(0)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(last, -1);
    let equal = engine
        .result_stack()
        .peek(1)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(equal, 0);
    let second = engine
        .result_stack()
        .peek(2)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(second, -1);
    let first = engine
        .result_stack()
        .peek(3)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(first, -1);
}

#[test]
fn stdlib_base58_check_encode_decode() {
    let stdlib = StdLib::new();
    let mut sb = ScriptBuilder::new();
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "base58CheckEncode",
        vec![StackItem::from_byte_string(vec![1, 2, 3])],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "base58CheckDecode",
        vec![StackItem::from_byte_string(b"3DUz7ncyT".to_vec())],
    );
    sb.emit_opcode(OpCode::RET);

    let mut engine = make_engine();
    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.result_stack().len(), 2);
    let decoded = engine.result_stack().peek(0).unwrap().as_bytes().unwrap();
    assert_eq!(decoded, vec![1, 2, 3]);
    let encoded = engine.result_stack().peek(1).unwrap().as_bytes().unwrap();
    assert_eq!(String::from_utf8(encoded).unwrap(), "3DUz7ncyT");
}

#[test]
fn stdlib_base58_check_decode_faults_on_invalid() {
    let stdlib = StdLib::new();
    let cases = [
        StackItem::from_byte_string(b"AA".to_vec()),
        StackItem::null(),
    ];

    for arg in cases {
        let mut sb = ScriptBuilder::new();
        emit_stdlib_call(&mut sb, stdlib.hash(), "base58CheckDecode", vec![arg]);
        sb.emit_opcode(OpCode::RET);

        let mut engine = make_engine();
        engine
            .load_script(sb.to_array(), CallFlags::ALL, None)
            .expect("load script");
        assert!(engine.execute().is_err());
    }
}

#[test]
fn stdlib_memory_search_string_parity() {
    let stdlib = StdLib::new();
    let mut sb = ScriptBuilder::new();
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"c".to_vec()),
            StackItem::from_int(0),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"c".to_vec()),
            StackItem::from_int(1),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"c".to_vec()),
            StackItem::from_int(2),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"c".to_vec()),
            StackItem::from_int(3),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"d".to_vec()),
            StackItem::from_int(0),
        ],
    );
    sb.emit_opcode(OpCode::RET);

    let mut engine = make_engine();
    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.result_stack().len(), 5);
    let last = engine
        .result_stack()
        .peek(0)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(last, -1);
    let start3 = engine
        .result_stack()
        .peek(1)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(start3, -1);
    let start2 = engine
        .result_stack()
        .peek(2)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(start2, 2);
    let start1 = engine
        .result_stack()
        .peek(3)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(start1, 2);
    let start0 = engine
        .result_stack()
        .peek(4)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(start0, 2);
}

#[test]
fn stdlib_memory_search_backward_flags() {
    let stdlib = StdLib::new();
    let mut sb = ScriptBuilder::new();
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"c".to_vec()),
            StackItem::from_int(0),
            StackItem::from_bool(false),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"c".to_vec()),
            StackItem::from_int(1),
            StackItem::from_bool(false),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"c".to_vec()),
            StackItem::from_int(2),
            StackItem::from_bool(false),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"c".to_vec()),
            StackItem::from_int(3),
            StackItem::from_bool(false),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"d".to_vec()),
            StackItem::from_int(0),
            StackItem::from_bool(false),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"c".to_vec()),
            StackItem::from_int(0),
            StackItem::from_bool(true),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"c".to_vec()),
            StackItem::from_int(1),
            StackItem::from_bool(true),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"c".to_vec()),
            StackItem::from_int(2),
            StackItem::from_bool(true),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"c".to_vec()),
            StackItem::from_int(3),
            StackItem::from_bool(true),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(b"abc".to_vec()),
            StackItem::from_byte_string(b"d".to_vec()),
            StackItem::from_int(0),
            StackItem::from_bool(true),
        ],
    );
    sb.emit_opcode(OpCode::RET);

    let mut engine = make_engine();
    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.result_stack().len(), 10);
    let backward_last = engine
        .result_stack()
        .peek(0)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(backward_last, -1);
    let backward_start3 = engine
        .result_stack()
        .peek(1)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(backward_start3, 2);
    let backward_start2 = engine
        .result_stack()
        .peek(2)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(backward_start2, -1);
    let backward_start1 = engine
        .result_stack()
        .peek(3)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(backward_start1, -1);
    let backward_start0 = engine
        .result_stack()
        .peek(4)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(backward_start0, -1);

    let forward_last = engine
        .result_stack()
        .peek(5)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(forward_last, -1);
    let forward_start3 = engine
        .result_stack()
        .peek(6)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(forward_start3, -1);
    let forward_start2 = engine
        .result_stack()
        .peek(7)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(forward_start2, 2);
    let forward_start1 = engine
        .result_stack()
        .peek(8)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(forward_start1, 2);
    let forward_start0 = engine
        .result_stack()
        .peek(9)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(forward_start0, 2);
}

#[test]
fn stdlib_string_split_and_length_parity() {
    let stdlib = StdLib::new();
    let mut sb = ScriptBuilder::new();
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "stringSplit",
        vec![
            StackItem::from_byte_string(b"a,b".to_vec()),
            StackItem::from_byte_string(b",".to_vec()),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "strLen",
        vec![StackItem::from_byte_string("🦆".as_bytes().to_vec())],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "strLen",
        vec![StackItem::from_byte_string("ã".as_bytes().to_vec())],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "strLen",
        vec![StackItem::from_byte_string(b"a".to_vec())],
    );
    sb.emit_opcode(OpCode::RET);

    let mut engine = make_engine();
    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.result_stack().len(), 4);
    let strlen_a = engine
        .result_stack()
        .peek(0)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(strlen_a, 1);
    let strlen_a_tilde = engine
        .result_stack()
        .peek(1)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(strlen_a_tilde, 1);
    let strlen_duck = engine
        .result_stack()
        .peek(2)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(strlen_duck, 1);
    let split_item = engine.result_stack().peek(3).unwrap();
    let parts = split_item.as_array().unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(
        String::from_utf8(parts[0].as_bytes().unwrap()).unwrap(),
        "a"
    );
    assert_eq!(
        String::from_utf8(parts[1].as_bytes().unwrap()).unwrap(),
        "b"
    );
}

#[test]
fn stdlib_strlen_handles_invalid_utf8_sequence() {
    let stdlib = StdLib::new();
    let mut sb = ScriptBuilder::new();
    let bad_str = "\u{00FF}";
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "strLen",
        vec![StackItem::from_byte_string(bad_str.as_bytes().to_vec())],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "strLen",
        vec![StackItem::from_byte_string(
            format!("{bad_str}ab").into_bytes(),
        )],
    );
    sb.emit_opcode(OpCode::RET);

    let mut engine = make_engine();
    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.result_stack().len(), 2);
    let strlen_ab = engine
        .result_stack()
        .peek(0)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(strlen_ab, 3);
    let strlen_bad = engine
        .result_stack()
        .peek(1)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(strlen_bad, 1);
}

#[test]
fn stdlib_json_deserialize_parity() {
    let stdlib = StdLib::new();
    let mut sb = ScriptBuilder::new();
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "jsonDeserialize",
        vec![StackItem::from_byte_string(b"123".to_vec())],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "jsonDeserialize",
        vec![StackItem::from_byte_string(b"null".to_vec())],
    );
    sb.emit_opcode(OpCode::RET);

    let mut engine = make_engine();
    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.result_stack().len(), 2);
    let null_item = engine.result_stack().peek(0).unwrap();
    assert!(null_item.is_null());
    let int_item = engine
        .result_stack()
        .peek(1)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(int_item, 123);
}

#[test]
fn stdlib_json_deserialize_faults_on_invalid() {
    let stdlib = StdLib::new();
    for payload in ["***", "123.45"] {
        let mut sb = ScriptBuilder::new();
        emit_stdlib_call(
            &mut sb,
            stdlib.hash(),
            "jsonDeserialize",
            vec![StackItem::from_byte_string(payload.as_bytes().to_vec())],
        );
        sb.emit_opcode(OpCode::RET);

        let mut engine = make_engine();
        engine
            .load_script(sb.to_array(), CallFlags::ALL, None)
            .expect("load script");
        assert!(engine.execute().is_err());
    }
}

#[test]
#[allow(clippy::mutable_key_type)]
fn stdlib_json_serialize_parity() {
    let stdlib = StdLib::new();
    let mut sb = ScriptBuilder::new();
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "jsonSerialize",
        vec![StackItem::from_int(5)],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "jsonSerialize",
        vec![StackItem::from_bool(true)],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "jsonSerialize",
        vec![StackItem::from_byte_string(b"test".to_vec())],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "jsonSerialize",
        vec![StackItem::null()],
    );

    let mut map = BTreeMap::new();
    map.insert(
        StackItem::from_byte_string(b"key".to_vec()),
        StackItem::from_byte_string(b"value".to_vec()),
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "jsonSerialize",
        vec![StackItem::from_map(map)],
    );
    sb.emit_opcode(OpCode::RET);

    let mut engine = make_engine();
    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.result_stack().len(), 5);
    let map_json = engine.result_stack().peek(0).unwrap().as_bytes().unwrap();
    assert_eq!(String::from_utf8(map_json).unwrap(), "{\"key\":\"value\"}");
    let null_json = engine.result_stack().peek(1).unwrap().as_bytes().unwrap();
    assert_eq!(String::from_utf8(null_json).unwrap(), "null");
    let string_json = engine.result_stack().peek(2).unwrap().as_bytes().unwrap();
    assert_eq!(String::from_utf8(string_json).unwrap(), "\"test\"");
    let bool_json = engine.result_stack().peek(3).unwrap().as_bytes().unwrap();
    assert_eq!(String::from_utf8(bool_json).unwrap(), "true");
    let int_json = engine.result_stack().peek(4).unwrap().as_bytes().unwrap();
    assert_eq!(String::from_utf8(int_json).unwrap(), "5");
}

#[test]
fn stdlib_json_serialize_faults_without_args() {
    let stdlib = StdLib::new();
    let mut sb = ScriptBuilder::new();
    emit_stdlib_call(&mut sb, stdlib.hash(), "jsonSerialize", vec![]);
    sb.emit_opcode(OpCode::RET);

    let mut engine = make_engine();
    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    assert!(engine.execute().is_err());
}

#[test]
fn stdlib_runtime_serialize_deserialize_parity() {
    let stdlib = StdLib::new();
    let mut sb = ScriptBuilder::new();
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "serialize",
        vec![StackItem::from_int(100)],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "serialize",
        vec![StackItem::from_byte_string(b"test".to_vec())],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "deserialize",
        vec![StackItem::from_byte_string(
            hex::decode("280474657374").unwrap(),
        )],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "deserialize",
        vec![StackItem::from_byte_string(hex::decode("210164").unwrap())],
    );
    sb.emit_opcode(OpCode::RET);

    let mut engine = make_engine();
    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.result_stack().len(), 4);
    let deserialized_int = engine
        .result_stack()
        .peek(0)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(deserialized_int, 100);
    let deserialized_string = engine.result_stack().peek(1).unwrap().as_bytes().unwrap();
    assert_eq!(String::from_utf8(deserialized_string).unwrap(), "test");
    let serialized_string = engine.result_stack().peek(2).unwrap().as_bytes().unwrap();
    assert_eq!(hex::encode(serialized_string), "280474657374");
    let serialized_int = engine.result_stack().peek(3).unwrap().as_bytes().unwrap();
    assert_eq!(hex::encode(serialized_int), "210164");
}

#[test]
fn stdlib_base64_url_parity() {
    let stdlib = StdLib::new();
    let mut sb = ScriptBuilder::new();
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "base64UrlEncode",
        vec![StackItem::from_byte_string(
            b"Subject=test@example.com&Issuer=https://example.com".to_vec(),
        )],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "base64UrlDecode",
        vec![StackItem::from_byte_string(
            b"U3ViamVjdD10ZXN0QGV4YW1wbGUuY29tJklzc3Vlcj1odHRwczovL2V4YW1wbGUuY29t".to_vec(),
        )],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "base64UrlDecode",
        vec![StackItem::from_byte_string(
            b"U 3 \t V \n \riamVjdD10ZXN0QGV4YW1wbGUuY29tJklzc3Vlcj1odHRwczovL2V4YW1wbGUuY29t"
                .to_vec(),
        )],
    );
    sb.emit_opcode(OpCode::RET);

    let mut engine = make_engine_with_height(Some(7_300_000));
    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.result_stack().len(), 3);
    let decoded_ws = engine.result_stack().peek(0).unwrap().as_bytes().unwrap();
    assert_eq!(
        String::from_utf8(decoded_ws).unwrap(),
        "Subject=test@example.com&Issuer=https://example.com"
    );
    let decoded = engine.result_stack().peek(1).unwrap().as_bytes().unwrap();
    assert_eq!(
        String::from_utf8(decoded).unwrap(),
        "Subject=test@example.com&Issuer=https://example.com"
    );
    let encoded = engine.result_stack().peek(2).unwrap().as_bytes().unwrap();
    assert_eq!(
        String::from_utf8(encoded).unwrap(),
        "U3ViamVjdD10ZXN0QGV4YW1wbGUuY29tJklzc3Vlcj1odHRwczovL2V4YW1wbGUuY29t"
    );
}

#[test]
fn stdlib_hex_encode_decode_parity() {
    let stdlib = StdLib::new();
    let mut sb = ScriptBuilder::new();
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "hexEncode",
        vec![StackItem::from_byte_string(vec![0x00, 0x01, 0x02, 0x03])],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "hexDecode",
        vec![StackItem::from_byte_string(b"00010203".to_vec())],
    );
    sb.emit_opcode(OpCode::RET);

    let mut settings = ProtocolSettings::default();
    let mut hardforks = HashMap::new();
    hardforks.insert(Hardfork::HfFaun, 0);
    settings.hardforks = hardforks;
    let mut engine = make_engine_with_settings(settings);
    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.result_stack().len(), 2);
    let decoded = engine.result_stack().peek(0).unwrap().as_bytes().unwrap();
    assert_eq!(decoded, vec![0x00, 0x01, 0x02, 0x03]);
    let encoded = engine.result_stack().peek(1).unwrap().as_bytes().unwrap();
    assert_eq!(String::from_utf8(encoded).unwrap(), "00010203");
}

#[test]
fn stdlib_memory_search_bytes_parity() {
    let stdlib = StdLib::new();
    let mut sb = ScriptBuilder::new();
    let mem = vec![0x00, 0x01, 0x02, 0x03];
    let value = vec![0x03];
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(mem.clone()),
            StackItem::from_byte_string(value.clone()),
            StackItem::from_int(0),
            StackItem::from_bool(false),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(mem.clone()),
            StackItem::from_byte_string(value.clone()),
            StackItem::from_int((mem.len() - 1) as i32),
            StackItem::from_bool(false),
        ],
    );
    emit_stdlib_call(
        &mut sb,
        stdlib.hash(),
        "memorySearch",
        vec![
            StackItem::from_byte_string(mem.clone()),
            StackItem::from_byte_string(value.clone()),
            StackItem::from_int(mem.len() as i32),
            StackItem::from_bool(true),
        ],
    );
    sb.emit_opcode(OpCode::RET);

    let mut engine = make_engine();
    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.result_stack().len(), 3);
    let backward = engine
        .result_stack()
        .peek(0)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(backward, 3);
    let forward_end = engine
        .result_stack()
        .peek(1)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(forward_end, 3);
    let forward = engine
        .result_stack()
        .peek(2)
        .unwrap()
        .as_int()
        .unwrap()
        .to_i32()
        .unwrap();
    assert_eq!(forward, 3);
}
