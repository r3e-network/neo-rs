//! Smart Contract Execution Integration Tests

use neo_primitives::UInt160;
use neo_tests::state::{MemoryWorldState, StateChanges, StorageItem, StorageKey, WorldState};
use neo_vm_rs::{ExecutionResult, OpCode, StackValue, VmState, interpret};
use num_bigint::BigInt;

fn run_script(script: &[u8]) -> ExecutionResult {
    interpret(script).expect("neo-vm-rs interpreter should execute script")
}

fn top_int(result: &ExecutionResult) -> BigInt {
    match result.stack.last().expect("result stack item") {
        StackValue::Integer(value) => BigInt::from(*value),
        StackValue::BigInteger(bytes) => BigInt::from_signed_bytes_le(bytes),
        item => panic!("expected integer stack value, got {item:?}"),
    }
}

#[tokio::test]
async fn test_vm_simple_push_and_return() {
    let script = vec![OpCode::PUSH1.byte(), OpCode::RET.byte()];
    let result = run_script(&script);
    assert_eq!(result.state, VmState::Halt);
    let val = top_int(&result);
    assert_eq!(val, BigInt::from(1), "PUSH1 should produce 1");
}

#[tokio::test]
async fn test_vm_arithmetic_add() {
    // PUSH2, PUSH3, ADD, RET
    let script = vec![
        OpCode::PUSH2.byte(),
        OpCode::PUSH3.byte(),
        OpCode::ADD.byte(),
        OpCode::RET.byte(),
    ];
    let result = run_script(&script);
    assert_eq!(result.state, VmState::Halt);
    let val = top_int(&result);
    assert_eq!(val, BigInt::from(5), "PUSH2 + PUSH3 should equal 5");
}

#[tokio::test]
async fn test_vm_arithmetic_sub() {
    let script = vec![
        OpCode::PUSH5.byte(),
        OpCode::PUSH8.byte(),
        OpCode::SUB.byte(),
        OpCode::RET.byte(),
    ];
    let result = run_script(&script);
    assert_eq!(result.state, VmState::Halt);
    let val = top_int(&result);
    assert_eq!(val, BigInt::from(-3), "PUSH5 - PUSH8 should equal -3");
}

#[tokio::test]
async fn test_vm_arithmetic_mul() {
    let script = vec![
        OpCode::PUSH6.byte(),
        OpCode::PUSH7.byte(),
        OpCode::MUL.byte(),
        OpCode::RET.byte(),
    ];
    let result = run_script(&script);
    assert_eq!(result.state, VmState::Halt);
    let val = top_int(&result);
    assert_eq!(val, BigInt::from(42), "PUSH6 * PUSH7 should equal 42");
}

#[tokio::test]
async fn test_contract_storage_basic() {
    let mut world_state = MemoryWorldState::new();
    let contract = UInt160::from([0x01u8; 20]);

    let key = StorageKey::new(contract, vec![0x01, 0x02]);
    let value = StorageItem::new(vec![0x00, 0x00, 0x00, 0x01]);

    let mut changes = StateChanges::new();
    changes.storage.insert(key.clone(), Some(value));
    world_state.commit(changes).unwrap();

    let retrieved = world_state.get_storage(&key).unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().as_bytes(), &[0x00, 0x00, 0x00, 0x01]);
}

#[tokio::test]
async fn test_contract_storage_update() {
    let mut world_state = MemoryWorldState::new();
    let contract = UInt160::from([0x01u8; 20]);
    let key = StorageKey::new(contract, vec![0x01]);

    let initial = StorageItem::new(vec![0x01]);
    let mut changes = StateChanges::new();
    changes.storage.insert(key.clone(), Some(initial));
    world_state.commit(changes).unwrap();

    let updated = StorageItem::new(vec![0x02]);
    let mut changes = StateChanges::new();
    changes.storage.insert(key.clone(), Some(updated));
    world_state.commit(changes).unwrap();

    let retrieved = world_state.get_storage(&key).unwrap().unwrap();
    assert_eq!(retrieved.as_bytes(), &[0x02]);
}

#[tokio::test]
async fn test_contract_storage_deletion() {
    let mut world_state = MemoryWorldState::new();
    let contract = UInt160::from([0x01u8; 20]);
    let key = StorageKey::new(contract, vec![0x01]);

    let value = StorageItem::new(vec![0x01]);
    let mut changes = StateChanges::new();
    changes.storage.insert(key.clone(), Some(value));
    world_state.commit(changes).unwrap();

    assert!(world_state.get_storage(&key).unwrap().is_some());

    let mut changes = StateChanges::new();
    changes.storage.insert(key.clone(), None);
    world_state.commit(changes).unwrap();

    assert!(world_state.get_storage(&key).unwrap().is_none());
}

#[tokio::test]
async fn test_vm_gas_consumption_basic() {
    let script = vec![
        OpCode::PUSH1.byte(),
        OpCode::PUSH2.byte(),
        OpCode::ADD.byte(),
        OpCode::RET.byte(),
    ];
    let result = run_script(&script);
    assert_eq!(result.state, VmState::Halt);
    assert_eq!(result.fee_consumed_pico, 0);
    assert!(result.fault_message.is_none());
}

#[tokio::test]
async fn test_vm_stack_underflow() {
    let script = vec![OpCode::ADD.byte(), OpCode::RET.byte()];
    assert!(interpret(&script).is_err());
}

#[tokio::test]
async fn test_vm_invalid_opcode() {
    let script = vec![0xFF, 0xFF, OpCode::RET.byte()];
    assert!(interpret(&script).is_err());
}

#[tokio::test]
async fn test_contract_state_consistency() {
    let mut world_state = MemoryWorldState::new();
    let contract = UInt160::from([0x01u8; 20]);

    let keys: Vec<_> = (0..5).map(|i| StorageKey::new(contract, vec![i])).collect();

    let mut changes = StateChanges::new();
    for (i, key) in keys.iter().enumerate() {
        let value = StorageItem::new(vec![i as u8]);
        changes.storage.insert(key.clone(), Some(value));
    }
    world_state.commit(changes).unwrap();

    for (i, key) in keys.iter().enumerate() {
        let value = world_state.get_storage(key).unwrap().unwrap();
        assert_eq!(value.as_bytes(), &[i as u8]);
    }
}

#[tokio::test]
async fn test_contract_storage_large_value() {
    let mut world_state = MemoryWorldState::new();
    let contract = UInt160::from([0x01u8; 20]);
    let key = StorageKey::new(contract, vec![0x01]);

    let large_value = vec![0xABu8; 65536];
    let item = StorageItem::new(large_value);

    let mut changes = StateChanges::new();
    changes.storage.insert(key.clone(), Some(item));
    world_state.commit(changes).unwrap();

    let retrieved = world_state.get_storage(&key).unwrap().unwrap();
    assert_eq!(retrieved.as_bytes().len(), 65536);
}

#[tokio::test]
async fn test_contract_storage_many_keys() {
    let mut world_state = MemoryWorldState::new();
    let contract = UInt160::from([0x01u8; 20]);

    let mut changes = StateChanges::new();
    for i in 0u32..1000 {
        let key = StorageKey::new(contract, i.to_le_bytes().to_vec());
        let value = StorageItem::new(vec![(i % 256) as u8]);
        changes.storage.insert(key, Some(value));
    }

    world_state.commit(changes).unwrap();

    for i in [0u32, 100, 500, 999] {
        let key = StorageKey::new(contract, i.to_le_bytes().to_vec());
        let value = world_state.get_storage(&key).unwrap().unwrap();
        assert_eq!(value.as_bytes(), &[(i % 256) as u8]);
    }
}
