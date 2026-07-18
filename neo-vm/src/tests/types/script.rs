use super::*;
use crate::OpCode;

#[test]
fn test_script_creation_and_validation() {
    let script_bytes = vec![
        OpCode::PUSH1.byte(),
        OpCode::PUSH2.byte(),
        OpCode::ADD.byte(),
        OpCode::RET.byte(),
    ];

    let script = Script::new_relaxed(script_bytes.clone());
    assert_eq!(script.len(), 4);

    let script = Script::new(script_bytes.clone(), true).unwrap();
    assert_eq!(script.len(), 4);

    let instr = script.get_instruction(0).unwrap();
    assert_eq!(instr.opcode(), OpCode::PUSH1);

    let instr = script.get_instruction(1).unwrap();
    assert_eq!(instr.opcode(), OpCode::PUSH2);

    assert_eq!(script.get_byte(0).unwrap(), OpCode::PUSH1.byte());
    assert_eq!(script.get_byte(1).unwrap(), OpCode::PUSH2.byte());

    assert_eq!(
        script.range(0, 2).expect("Operation failed"),
        vec![OpCode::PUSH1.byte(), OpCode::PUSH2.byte()]
    );

    let hash = script.hash_code();
    assert_ne!(hash, 0);

    let hash_bytes = script.hash();
    assert_eq!(hash_bytes.len(), 8);
}

#[test]
fn test_script_validation_with_invalid_jump() {
    let script_bytes = vec![OpCode::JMP.byte(), 0xFF];

    let script = Script::new_relaxed(script_bytes.clone());
    assert_eq!(script.len(), 2);

    let result = Script::new(script_bytes.clone(), true);
    assert!(result.is_err());
}

#[test]
fn test_script_instruction_caching() {
    let script_bytes = vec![
        OpCode::PUSH1.byte(),
        OpCode::PUSH2.byte(),
        OpCode::ADD.byte(),
        OpCode::RET.byte(),
    ];

    let script = Script::new_relaxed(script_bytes.clone());

    let instr1 = script.get_instruction(0).unwrap();
    assert_eq!(instr1.opcode(), OpCode::PUSH1);

    let instr2 = script.get_instruction(0).unwrap();
    assert_eq!(instr2.opcode(), OpCode::PUSH1);

    assert_eq!(instr1.pointer(), instr2.pointer());
    assert_eq!(instr1.opcode(), instr2.opcode());
}

#[test]
fn shared_bytes_reuses_the_script_allocation() {
    let script = Script::new_relaxed(vec![OpCode::PUSH1.byte(), OpCode::RET.byte()]);
    let first = script.shared_bytes();
    let second = script.shared_bytes();

    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(first.as_ref(), script.as_bytes());
}

#[test]
fn relaxed_script_cache_allocates_segments_on_demand() {
    let script = Script::new_relaxed(vec![OpCode::NOP.byte(); INSTRUCTION_CACHE_SEGMENT_SIZE * 3]);
    let InstructionCache::Lazy(cache) = &script.instructions else {
        panic!("relaxed scripts must use the lazy instruction cache");
    };
    assert_eq!(cache.initialized_segment_count(), 0);

    script.get_instruction(0).expect("first instruction");
    assert_eq!(cache.initialized_segment_count(), 1);

    script
        .get_instruction(INSTRUCTION_CACHE_SEGMENT_SIZE * 2)
        .expect("third segment instruction");
    assert_eq!(cache.initialized_segment_count(), 2);
}

#[test]
fn test_script_with_valid_jumps() {
    let script_bytes = vec![
        OpCode::PUSH1.byte(),
        OpCode::JMP.byte(),
        0x02,
        OpCode::PUSH2.byte(),
        OpCode::ADD.byte(),
        OpCode::RET.byte(),
    ];

    let script = Script::new(script_bytes.clone(), true).unwrap();

    let jump_instr = script.get_instruction(1).unwrap();
    let target = script.get_jump_target(&jump_instr).unwrap();

    assert_eq!(target, 3);

    let target_instr = script.get_instruction(target).unwrap();
    assert_eq!(target_instr.opcode(), OpCode::PUSH2);
}
