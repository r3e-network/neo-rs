use super::{OracleContract, PendingRequest};
use crate::error::{CoreError as Error, CoreResult as Result};
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::neo_vm::{StackItem};
use crate::UInt160;

impl OracleContract {
    pub(super) fn emit_oracle_request(
        &self,
        engine: &mut ApplicationEngine,
        id: u64,
        contract: UInt160,
        request: &PendingRequest,
    ) -> Result<()> {
        let id_i64 = i64::try_from(id)
            .map_err(|_| Error::runtime_error("Oracle request id exceeds i64::MAX"))?;
        let state = vec![
            StackItem::from_int(id_i64),
            StackItem::from_byte_string(contract.to_bytes()),
            StackItem::from_byte_string(request.url.as_bytes().to_vec()),
            match &request.filter {
                Some(filter) => StackItem::from_byte_string(filter.as_bytes().to_vec()),
                None => StackItem::null(),
            },
        ];
        engine
            .send_notification(self.hash, "OracleRequest".to_string(), state)
            .map_err(Error::runtime_error)
    }

    pub(super) fn emit_oracle_response(
        &self,
        engine: &mut ApplicationEngine,
        request_id: u64,
        request: &PendingRequest,
    ) -> Result<()> {
        let id_i64 = i64::try_from(request_id)
            .map_err(|_| Error::runtime_error("Oracle request id exceeds i64::MAX"))?;
        let state = vec![
            StackItem::from_int(id_i64),
            StackItem::from_byte_string(request.original_tx_id.to_bytes()),
        ];
        engine
            .send_notification(self.hash, "OracleResponse".to_string(), state)
            .map_err(Error::runtime_error)
    }
}
