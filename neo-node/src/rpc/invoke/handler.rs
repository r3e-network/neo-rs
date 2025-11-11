use neo_contract::{ApplicationEngine, EngineConfig};
use neo_core::script::Script;
use neo_rpc::{RpcError, RpcModule};
use neo_store::{ColumnId, MemoryStore};

use super::{format::invocation_to_json, parse::parse_script_bytes};

pub fn register_invocation_methods(module: &RpcModule) {
    module.register("invokescript", move |params| async move {
        let script_bytes = parse_script_bytes(&params)?;
        let script = Script::new(script_bytes);
        let mut store = MemoryStore::new();
        store.create_column(ColumnId::new("contract"));
        let mut engine = ApplicationEngine::new(&mut store, EngineConfig::default(), None);
        let result = engine
            .execute_script(&script)
            .map_err(|err| RpcError::internal_error(format!("vm execution failed: {err:?}")))?;
        Ok(invocation_to_json(result))
    });
}
