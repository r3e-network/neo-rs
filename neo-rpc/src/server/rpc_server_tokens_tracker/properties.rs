//! NEP-11 property RPC handler.
//!
//! The handler invokes the target contract's `properties` method through the
//! application engine and translates the returned map into the Neo JSON-RPC
//! compatibility shape.

use std::sync::Arc;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::RpcServer;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_execution::application_engine::TEST_MODE_GAS;
use neo_execution::{ApplicationEngine, TriggerType};
use neo_manifest::CallFlags;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm::stack_item::StackItem;
use neo_vm_rs::VmState as VMState;
use serde_json::{Map, Value};

use super::RpcServerTokensTracker;
use super::helpers::{emit_contract_call_with_arg, tracker_service};
use super::request::Nep11PropertiesRequest;

const NEP11_PROPERTIES: [&str; 4] = ["name", "description", "image", "tokenURI"];

impl RpcServerTokensTracker {
    pub(super) fn get_nep11_properties(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let service = tracker_service(server)?;
        if !service.settings().enabled_nep11() {
            return Err(RpcException::from(RpcError::method_not_found()));
        }

        let address_version = server.system().settings().address_version;
        let request = Nep11PropertiesRequest::parse(params, address_version)?;

        let mut script = ScriptBuilder::new();
        emit_contract_call_with_arg(
            &mut script,
            &request.script_hash,
            "properties",
            CallFlags::READ_ONLY,
            &request.token_id,
        )?;

        let system = server.system();
        let store_cache = system.store_cache();
        let snapshot = Arc::new(store_cache.data_cache().clone());
        let mut engine = ApplicationEngine::new_with_shared_block_and_native_contract_provider(
            TriggerType::Application,
            None,
            snapshot,
            None,
            system.settings().as_ref().clone(),
            TEST_MODE_GAS,
            neo_execution::NoDiagnostic,
            system.native_contract_provider(),
        )
        .map_err(|err| internal_error(err.to_string()))?;
        engine
            .load_script(script.to_array(), CallFlags::ALL, Some(request.script_hash))
            .map_err(|err| internal_error(err.to_string()))?;
        engine
            .execute()
            .map_err(|err| internal_error(err.to_string()))?;

        if engine.state() != VMState::HALT {
            return Ok(Value::Object(Map::new()));
        }

        let map_item = engine
            .result_stack()
            .peek(0)
            .map_err(|err| internal_error(err.to_string()))?
            .clone();
        let map = map_item
            .as_map()
            .map_err(|err| internal_error(err.to_string()))?;

        let mut result = Map::new();
        for (key, value) in map.iter() {
            if matches!(
                value,
                StackItem::Array(_) | StackItem::Struct(_) | StackItem::Map(_)
            ) {
                continue;
            }

            let key_bytes = key
                .as_bytes()
                .map_err(|_| internal_error("unexpected null key"))?;
            let key_text =
                String::from_utf8(key_bytes).map_err(|err| internal_error(err.to_string()))?;

            if NEP11_PROPERTIES.iter().any(|prop| *prop == key_text) {
                if matches!(value, StackItem::Null) {
                    result.insert(key_text, Value::Null);
                } else {
                    let value_bytes = value
                        .as_bytes()
                        .map_err(|err| internal_error(err.to_string()))?;
                    let text = String::from_utf8(value_bytes)
                        .map_err(|err| internal_error(err.to_string()))?;
                    result.insert(key_text, Value::String(text));
                }
            } else if matches!(value, StackItem::Null) {
                result.insert(key_text, Value::Null);
            } else {
                let value_bytes = value
                    .as_bytes()
                    .map_err(|err| internal_error(err.to_string()))?;
                let encoded = BASE64_STANDARD.encode(value_bytes);
                result.insert(key_text, Value::String(encoded));
            }
        }

        Ok(Value::Object(result))
    }
}
