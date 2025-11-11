use super::{ApplicationEngine, EngineConfig};
use crate::runtime::Value;
use neo_core::script::{OpCode, ScriptBuilder};
use neo_store::{ColumnId, MemoryStore};

#[test]
fn engine_placeholder_executes() {
    let mut store = MemoryStore::new();
    store.create_column(ColumnId::new("contract"));
    let mut engine = ApplicationEngine::new(&mut store, EngineConfig::default(), None);
    let mut builder = ScriptBuilder::new();
    builder
        .push_int(1)
        .push_int(2)
        .push_opcode(OpCode::Add)
        .push_opcode(OpCode::Return);
    let script = builder.into_script();
    let result = engine.execute_script(&script).expect("exec succeeds");
    assert_eq!(result.value, Value::Int(3));
    assert_eq!(result.gas_used, 0);
}
