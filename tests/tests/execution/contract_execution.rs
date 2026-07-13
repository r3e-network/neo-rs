//! Smart Contract Execution Integration Tests

use neo_primitives::UInt160;
use neo_tests::state::{MemoryWorldState, StateChanges, StorageItem, StorageKey, WorldState};
use neo_vm::{ExecutionEngine, OpCode, Script, StackItem, VmError, VmState};
use num_bigint::BigInt;

struct ExecutionResult {
    state: VmState,
    stack: Vec<StackItem>,
    fee_consumed_pico: i64,
    fault_message: Option<String>,
}

fn try_run_script(script: &[u8]) -> Result<ExecutionResult, VmError> {
    let script = Script::new(script.to_vec(), true)?;
    let mut engine = ExecutionEngine::<()>::new(None);
    engine.load_script(script, -1, 0)?;
    let state = engine.execute();
    if state == VmState::FAULT {
        let message = engine
            .uncaught_exception()
            .and_then(|item| item.as_bytes().ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_else(|| "local neo-vm execution faulted".to_string());
        return Err(VmError::invalid_operation_msg(message));
    }

    Ok(ExecutionResult {
        state,
        stack: engine.result_stack().to_vec(),
        fee_consumed_pico: i64::try_from(engine.gas_consumed()).unwrap_or(i64::MAX),
        fault_message: None,
    })
}

fn run_script(script: &[u8]) -> ExecutionResult {
    try_run_script(script).expect("local neo-vm should execute script")
}

fn top_int(result: &ExecutionResult) -> BigInt {
    result
        .stack
        .last()
        .expect("result stack item")
        .as_int()
        .expect("expected integer stack item")
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
    assert!(try_run_script(&script).is_err());
}

#[tokio::test]
async fn test_vm_invalid_opcode() {
    let script = vec![0xFF, 0xFF, OpCode::RET.byte()];
    assert!(try_run_script(&script).is_err());
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
