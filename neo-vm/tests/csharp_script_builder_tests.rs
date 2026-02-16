use neo_vm::op_code::OpCode;
use neo_vm::script_builder::ScriptBuilder;
use num_bigint::BigInt;

fn to_hex(bytes: &[u8]) -> String {
    let mut result = String::from("0x");
    for b in bytes {
        result.push_str(&format!("{:02x}", b));
    }
    result
}

fn deterministic_bytes(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i & 0xFF) as u8).collect()
}

#[test]
fn test_emit() {
    let mut builder = ScriptBuilder::new();
    assert_eq!(builder.len(), 0);
    builder.emit_opcode(OpCode::NOP);
    assert_eq!(builder.len(), 1);
    assert_eq!(builder.to_array(), vec![OpCode::NOP as u8]);

    let mut builder = ScriptBuilder::new();
    builder.emit_instruction(OpCode::NOP, &[0x66]);
    assert_eq!(builder.to_array(), vec![OpCode::NOP as u8, 0x66]);
}

#[test]
fn test_null_and_empty() {
    let mut builder = ScriptBuilder::new();
    builder.emit_push(&[]);
    builder.emit_push(&[]);
    assert_eq!(
        builder.to_array(),
        vec![OpCode::PUSHDATA1 as u8, 0, OpCode::PUSHDATA1 as u8, 0]
    );
}

#[test]
fn test_big_integer() {
    let mut builder = ScriptBuilder::new();
    builder
        .emit_push_bigint(BigInt::from(-100_000))
        .expect("emit_push_bigint");
    assert_eq!(builder.len(), 5);
    assert_eq!(builder.to_array(), vec![0x02, 0x60, 0x79, 0xFE, 0xFF]);

    let mut builder = ScriptBuilder::new();
    builder
        .emit_push_bigint(BigInt::from(100_000))
        .expect("emit_push_bigint");
    assert_eq!(builder.len(), 5);
    assert_eq!(builder.to_array(), vec![0x02, 0xA0, 0x86, 0x01, 0x00]);
}

#[test]
fn test_emit_syscall() {
    let mut builder = ScriptBuilder::new();
    builder.emit_syscall_hash(0xE393C875);
    assert_eq!(
        builder.to_array(),
        vec![OpCode::SYSCALL as u8, 0x75, 0xC8, 0x93, 0xE3]
    );
}

#[test]
fn test_emit_call() {
    let mut builder = ScriptBuilder::new();
    builder.emit_call(0).expect("emit_call short");
    assert_eq!(builder.to_array(), vec![OpCode::CALL as u8, 0]);

    let mut builder = ScriptBuilder::new();
    builder.emit_call(12_345).expect("emit_call long");
    let mut expected = vec![OpCode::CALL_L as u8];
    expected.extend_from_slice(&12_345i32.to_le_bytes());
    assert_eq!(builder.to_array(), expected);

    let mut builder = ScriptBuilder::new();
    builder.emit_call(-12_345).expect("emit_call long negative");
    let mut expected = vec![OpCode::CALL_L as u8];
    expected.extend_from_slice(&(-12_345i32).to_le_bytes());
    assert_eq!(builder.to_array(), expected);
}

#[test]
fn test_emit_jump() {
    let offset_i8 = i8::MAX as i32;
    let offset_i32 = i32::MAX;

    for opcode in OpCode::iter() {
        let mut builder = ScriptBuilder::new();
        if !matches_jump_opcode(opcode) {
            assert!(builder.emit_jump(opcode, offset_i8).is_err());
            assert!(builder.emit_jump(opcode, offset_i32).is_err());
            continue;
        }

        builder
            .emit_jump(opcode, offset_i8)
            .expect("emit_jump short");
        builder
            .emit_jump(opcode, offset_i32)
            .expect("emit_jump long");

        let expected = expected_jump_sequence(opcode, offset_i8, offset_i32);
        assert_eq!(builder.to_array(), expected);
    }

    let offset_i8 = i8::MIN as i32;
    let offset_i32 = i32::MIN;

    for opcode in OpCode::iter() {
        let mut builder = ScriptBuilder::new();
        if !matches_jump_opcode(opcode) {
            assert!(builder.emit_jump(opcode, offset_i8).is_err());
            assert!(builder.emit_jump(opcode, offset_i32).is_err());
            continue;
        }

        builder
            .emit_jump(opcode, offset_i8)
            .expect("emit_jump short");
        builder
            .emit_jump(opcode, offset_i32)
            .expect("emit_jump long");

        let expected = expected_jump_sequence(opcode, offset_i8, offset_i32);
        assert_eq!(builder.to_array(), expected);
    }
}

#[test]
fn test_emit_push_big_integer_extended() {
    for value in -1i32..=16 {
        let mut builder = ScriptBuilder::new();
        builder
            .emit_push_bigint(BigInt::from(value))
            .expect("emit_push_bigint");
        let expected = if value == -1 {
            vec![OpCode::PUSHM1 as u8]
        } else {
            vec![(OpCode::PUSH0 as u8).wrapping_add(value as u8)]
        };
        assert_eq!(builder.to_array(), expected);
    }

    assert_eq!(
        to_hex(
            &ScriptBuilder::new()
                .emit_push_bigint(BigInt::from(-1))
                .expect("emit_push_bigint")
                .to_array()
        ),
        "0x0f"
    );

    let cases = [
        (BigInt::from(i8::MIN), "0x0080"),
        (BigInt::from(i8::MAX), "0x007f"),
        (BigInt::from(i16::MIN), "0x010080"),
        (BigInt::from(i16::MAX), "0x01ff7f"),
        (BigInt::from(i32::MIN), "0x0200000080"),
        (BigInt::from(i32::MAX), "0x02ffffff7f"),
        (BigInt::from(i64::MIN), "0x030000000000000080"),
        (BigInt::from(i64::MAX), "0x03ffffffffffffff7f"),
    ];

    for (value, expected_hex) in cases {
        let mut builder = ScriptBuilder::new();
        builder.emit_push_bigint(value).expect("emit_push_bigint");
        assert_eq!(to_hex(&builder.to_array()), expected_hex);
    }

    let ulong_max = BigInt::from(u64::MAX);
    assert_eq!(
        to_hex(
            &ScriptBuilder::new()
                .emit_push_bigint(ulong_max.clone())
                .expect("emit_push_bigint")
                .to_array()
        ),
        "0x04ffffffffffffffff0000000000000000"
    );

    assert_eq!(
        to_hex(
            &ScriptBuilder::new()
                .emit_push_bigint(ulong_max + 1)
                .expect("emit_push_bigint")
                .to_array()
        ),
        "0x0400000000000000000100000000000000"
    );

    let signed_min_256 = BigInt::parse_bytes(
        b"-57896044618658097711785492504343953926634992332820282019728792003956564819968",
        10,
    )
    .expect("parse big int");
    assert_eq!(
        to_hex(
            &ScriptBuilder::new()
                .emit_push_bigint(signed_min_256.clone())
                .expect("emit_push_bigint")
                .to_array()
        ),
        "0x050000000000000000000000000000000000000000000000000000000000000080"
    );

    let signed_max_256 = BigInt::parse_bytes(
        b"57896044618658097711785492504343953926634992332820282019728792003956564819967",
        10,
    )
    .expect("parse big int");
    assert_eq!(
        to_hex(
            &ScriptBuilder::new()
                .emit_push_bigint(signed_max_256.clone())
                .expect("emit_push_bigint")
                .to_array()
        ),
        "0x05ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f"
    );

    let exceed_256 = BigInt::parse_bytes(
        b"115792089237316195423570985008687907853269984665640564039457584007913129639936",
        10,
    )
    .expect("parse big int");
    assert!(ScriptBuilder::new().emit_push_bigint(exceed_256).is_err());

    assert_eq!(
        to_hex(
            &ScriptBuilder::new()
                .emit_push_bigint(BigInt::from(-2))
                .expect("emit_push_bigint")
                .to_array()
        ),
        "0x00fe"
    );
    assert_eq!(
        to_hex(
            &ScriptBuilder::new()
                .emit_push_bigint(BigInt::from(-256))
                .expect("emit_push_bigint")
                .to_array()
        ),
        "0x0100ff"
    );

    assert_eq!(
        to_hex(
            &ScriptBuilder::new()
                .emit_push_bigint(BigInt::parse_bytes(b"18446744073709551615", 10).unwrap())
                .expect("emit_push_bigint")
                .to_array()
        ),
        "0x04ffffffffffffffff0000000000000000"
    );

    assert_eq!(
        to_hex(
            &ScriptBuilder::new()
                .emit_push_bigint(BigInt::parse_bytes(b"18446744073709551616", 10).unwrap())
                .expect("emit_push_bigint")
                .to_array()
        ),
        "0x0400000000000000000100000000000000"
    );

    assert_eq!(
        to_hex(
            &ScriptBuilder::new()
                .emit_push_bigint(BigInt::parse_bytes(b"-18446744073709551616", 10).unwrap())
                .expect("emit_push_bigint")
                .to_array()
        ),
        "0x040000000000000000ffffffffffffffff"
    );

    let exceed_256_plus_one = BigInt::parse_bytes(
        b"115792089237316195423570985008687907853269984665640564039457584007913129639937",
        10,
    )
    .expect("parse big int");
    assert!(
        ScriptBuilder::new()
            .emit_push_bigint(exceed_256_plus_one)
            .is_err()
    );
}

#[test]
fn test_emit_push_bool() {
    let mut builder = ScriptBuilder::new();
    builder.emit_push_bool(true);
    assert_eq!(builder.to_array(), vec![OpCode::PUSHT as u8]);

    let mut builder = ScriptBuilder::new();
    builder.emit_push_bool(false);
    assert_eq!(builder.to_array(), vec![OpCode::PUSHF as u8]);
}

#[test]
fn test_emit_push_read_only_span() {
    let mut builder = ScriptBuilder::new();
    let data = [0x01u8, 0x02];
    builder.emit_push(&data);

    let mut expected = vec![OpCode::PUSHDATA1 as u8, data.len() as u8];
    expected.extend_from_slice(&data);
    assert_eq!(builder.to_array(), expected);
}

#[test]
fn test_emit_push_byte_array() {
    let mut builder = ScriptBuilder::new();
    builder.emit_push(&[]);
    assert_eq!(builder.to_array(), vec![OpCode::PUSHDATA1 as u8, 0]);

    let data = deterministic_bytes(0x4C);
    let mut builder = ScriptBuilder::new();
    builder.emit_push(&data);
    let mut expected = vec![OpCode::PUSHDATA1 as u8, data.len() as u8];
    expected.extend_from_slice(&data);
    assert_eq!(builder.to_array(), expected);

    let data = deterministic_bytes(0x100);
    let mut builder = ScriptBuilder::new();
    builder.emit_push(&data);
    let mut expected = vec![OpCode::PUSHDATA2 as u8];
    expected.extend_from_slice(&(data.len() as u16).to_le_bytes());
    expected.extend_from_slice(&data);
    assert_eq!(builder.to_array(), expected);

    let data = deterministic_bytes(0x10000);
    let mut builder = ScriptBuilder::new();
    builder.emit_push(&data);
    let mut expected = vec![OpCode::PUSHDATA4 as u8];
    expected.extend_from_slice(&(data.len() as u32).to_le_bytes());
    expected.extend_from_slice(&data);
    assert_eq!(builder.to_array(), expected);
}

#[test]
fn test_emit_push_string() {
    let data_short = "a".repeat(0x4C);
    let mut builder = ScriptBuilder::new();
    builder.emit_push_string(&data_short);
    let mut expected = vec![OpCode::PUSHDATA1 as u8, data_short.len() as u8];
    expected.extend_from_slice(data_short.as_bytes());
    assert_eq!(builder.to_array(), expected);

    let data_medium = "b".repeat(0x100);
    let mut builder = ScriptBuilder::new();
    builder.emit_push_string(&data_medium);
    let mut expected = vec![OpCode::PUSHDATA2 as u8];
    expected.extend_from_slice(&(data_medium.len() as u16).to_le_bytes());
    expected.extend_from_slice(data_medium.as_bytes());
    assert_eq!(builder.to_array(), expected);

    let data_large = "c".repeat(0x10000);
    let mut builder = ScriptBuilder::new();
    builder.emit_push_string(&data_large);
    let mut expected = vec![OpCode::PUSHDATA4 as u8];
    expected.extend_from_slice(&(data_large.len() as u32).to_le_bytes());
    expected.extend_from_slice(data_large.as_bytes());
    assert_eq!(builder.to_array(), expected);
}

fn matches_jump_opcode(opcode: OpCode) -> bool {
    let code = opcode as u8;
    let min = OpCode::JMP as u8;
    let max = OpCode::JMPLE_L as u8;
    code >= min && code <= max
}

fn expected_jump_sequence(opcode: OpCode, offset_i8: i32, offset_i32: i32) -> Vec<u8> {
    let mut result = Vec::new();
    let code = opcode as u8;
    if code % 2 == 0 {
        result.push(code);
        result.push((offset_i8 as i8) as u8);
        result.push(code + 1);
        result.extend_from_slice(&offset_i32.to_le_bytes());
    } else {
        result.push(code);
        result.extend_from_slice(&offset_i8.to_le_bytes());
        result.push(code);
        result.extend_from_slice(&offset_i32.to_le_bytes());
    }
    result
}
