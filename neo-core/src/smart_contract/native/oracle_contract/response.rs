use super::OracleContract;
use crate::error::{CoreError as Error, CoreResult as Result};
use crate::network::p2p::payloads::{
    oracle_response::OracleResponse as TxOracleResponse,
    transaction_attribute::TransactionAttribute,
};
use crate::persistence::DataCache;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::neo_vm::StackItem;

impl OracleContract {
    pub(super) fn finish(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        if engine.invocation_stack().len() != 2 {
            return Err(Error::invalid_operation(
                "Oracle finish must be invoked from the fixed response script".to_string(),
            ));
        }
        if engine.get_invocation_counter(&self.hash) != 1 {
            return Err(Error::invalid_operation(
                "Oracle finish cannot be re-entered".to_string(),
            ));
        }

        let tx = engine
            .script_container()
            .and_then(|container| {
                container
                    .as_any()
                    .downcast_ref::<crate::network::p2p::payloads::Transaction>()
            })
            .ok_or_else(|| {
                Error::invalid_operation(
                    "Oracle finish must be invoked within a transaction".to_string(),
                )
            })?;

        let response = tx
            .attributes()
            .iter()
            .find_map(|attr| match attr {
                TransactionAttribute::OracleResponse(attr) => Some(attr.clone()),
                _ => None,
            })
            .ok_or_else(|| Error::invalid_operation("Oracle response attribute missing"))?;

        let snapshot_arc = engine.snapshot_cache();
        let snapshot = snapshot_arc.as_ref();
        self.process_response(engine, snapshot, response)
    }

    fn process_response(
        &self,
        engine: &mut ApplicationEngine,
        snapshot: &DataCache,
        response: TxOracleResponse,
    ) -> Result<Vec<u8>> {
        let TxOracleResponse { id, code, result } = response;
        if result.len() > self.config.max_response_length {
            return Err(Error::invalid_operation(
                "Response data too long".to_string(),
            ));
        }
        let request = self
            .read_request(snapshot, id)?
            .ok_or_else(|| Error::invalid_operation("Request not found"))?;
        self.emit_oracle_response(engine, id, &request)?;

        let reference_counter = engine
            .current_context()
            .map(|context| context.reference_counter().clone());
        let user_data = BinarySerializer::deserialize(
            &request.user_data,
            engine.execution_limits(),
            reference_counter,
        )
        .map_err(Error::invalid_operation)?;

        engine.call_from_native_contract_dynamic(
            &self.hash,
            &request.callback_contract,
            &request.callback_method,
            vec![
                StackItem::from_byte_string(request.url.into_bytes()),
                user_data,
                StackItem::from_int(code as i32),
                StackItem::from_byte_string(result),
            ],
        )?;

        Ok(Vec::new())
    }
}
