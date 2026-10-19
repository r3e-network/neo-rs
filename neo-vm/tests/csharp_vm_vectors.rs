//! Differential vectors captured from the C# Neo.VM 3.10.1 `ExecutionEngine`.
//!
//! Each vector was produced by the reference implementation with an empty
//! initial stack and a default `ExecutionEngine`, then compared against the
//! Rust interpreter. This is a small, dependency-free C# parity check that
//! does not require the C# toolchain at test time.

use neo_vm::{ExecutionEngine, Script, StackValue, VmState, interpret};

fn format_value(value: &StackValue) -> String {
    match value {
        StackValue::Integer(value) => format!("int:{value}"),
        StackValue::BigInteger(bytes) => {
            format!("int:{}", num_bigint::BigInt::from_signed_bytes_le(bytes))
        }
        StackValue::Boolean(value) => format!("bool:{}", u8::from(*value)),
        StackValue::ByteString(bytes) | StackValue::Buffer(bytes) => {
            format!("bytes:{}", hex::encode(bytes))
        }
        StackValue::Array(items) => format!(
            "array:[{}]",
            items.iter().map(format_value).collect::<Vec<_>>().join(",")
        ),
        StackValue::Struct(items) => format!(
            "struct:[{}]",
            items.iter().map(format_value).collect::<Vec<_>>().join(",")
        ),
        StackValue::Map(pairs) => {
            let mut entries = pairs
                .iter()
                .map(|(key, value)| format!("{}:{}", format_value(key), format_value(value)))
                .collect::<Vec<_>>();
            entries.sort();
            format!("map:{{{}}}", entries.join(","))
        }
        StackValue::Null => "null".to_string(),
        other => format!("type:{:?}", other.compact_type_tag()),
    }
}

#[test]
fn csharp_vm_arithmetic_and_comparison_vectors_match() {
    // script | expected state | expected stack
    let vectors = [
        ("11119e40", "HALT", "int:2"),            // PUSH1 PUSH1 ADD RET
        ("12129e40", "HALT", "int:4"),            // PUSH2 PUSH2 ADD RET
        ("11129f40", "HALT", "int:-1"),           // PUSH1 PUSH2 SUB RET
        ("11129740", "HALT", "bool:0"),           // PUSH1 PUSH2 EQUAL RET
        ("11129840", "HALT", "bool:1"),           // PUSH1 PUSH2 NOTEQUAL RET
        ("1112b540", "HALT", "bool:1"),           // PUSH1 PUSH2 LT RET
        ("1112b640", "HALT", "bool:1"),           // PUSH1 PUSH2 LE RET
        ("1212b740", "HALT", "bool:0"),           // PUSH2 PUSH2 GT RET
        ("1112b840", "HALT", "bool:0"),           // PUSH1 PUSH2 GE RET
        ("1113a340", "HALT", "int:1"),            // PUSH1 PUSH3 POW RET
        ("1110a240", "FAULT", ""),                // PUSH1 PUSH0 MOD RET
        ("11129340", "HALT", "int:3"),            // PUSH1 PUSH2 XOR RET
        ("0c0361626340", "HALT", "bytes:616263"), // PUSHDATA1 "abc" RET
        ("0c036162630c036465668b40", "HALT", "bytes:616263646566"), // CAT "abc" "def"
        ("0c0361626311128c40", "HALT", "bytes:6263"), // SUBSTR "abc" 1 2
        ("0c03616263118d40", "HALT", "bytes:61"), // LEFT "abc" 1
        ("0c03616263118e40", "HALT", "bytes:63"), // RIGHT "abc" 1
        ("0c03616263ca40", "HALT", "int:3"),      // SIZE "abc"
        ("34054021211140", "HALT", "int:1"),      // CALL subtask; RET; NOP NOP; PUSH1; RET
        ("c240", "HALT", "array:[]"),             // NEWARRAY0 RET
        ("11c340", "HALT", "array:[null]"),       // PUSH1 NEWARRAY RET
        ("c24a11cf40", "HALT", "array:[int:1]"),  // NEWARRAY0 DUP PUSH1 APPEND RET
        ("c24a11cfca40", "HALT", "int:1"),        // NEWARRAY0 DUP PUSH1 APPEND SIZE RET
        ("11c310ce40", "HALT", "null"),           // PUSH1 NEWARRAY PUSH0 PICKITEM RET
        ("111213bb40", "HALT", "bool:0"),         // PUSH1 PUSH2 PUSH3 WITHIN RET
        ("121113bb40", "HALT", "bool:1"),         // PUSH2 PUSH1 PUSH3 WITHIN RET
    ];

    for (hex, expected_state, expected_stack) in vectors {
        let script = hex::decode(hex).expect("valid script hex");
        let (state, stack) = match interpret(&script) {
            Ok(result) => {
                let state = match result.state {
                    VmState::Halt => "HALT",
                    VmState::Fault => "FAULT",
                    other => panic!("{hex}: unexpected VM state {other:?}"),
                };
                let stack = result
                    .stack
                    .iter()
                    .map(format_value)
                    .collect::<Vec<_>>()
                    .join(",");
                (state, stack)
            }
            Err(_) => ("FAULT", String::new()),
        };
        assert_eq!(state, expected_state, "{hex}: state");
        assert_eq!(stack, expected_stack, "{hex}: stack");
    }
}

#[test]
fn csharp_newarray_reference_count_matches() {
    // C# 3.10.1: after PUSHINT16 1000; NEWARRAY the reference counter is
    // 1001 (the array plus its 1000 null children).
    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(Script::new_relaxed(hex::decode("01e803c3").unwrap()), -1, 0)
        .expect("load script");
    engine.execute_next().expect("push count");
    engine.execute_next().expect("newarray");
    assert_eq!(engine.reference_counter().count(), 1001);
}
