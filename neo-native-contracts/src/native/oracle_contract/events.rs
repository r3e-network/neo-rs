//! Oracle event emission.
//!
//! Migrated from `neo-core/src/smart_contract/native/oracle_contract/events.rs`.
//! Import adaptation: `crate::UInt160` → `neo_primitives::UInt160`;
//! `crate::error::*` → `neo_smart_contract::error::*`; `crate::neo_vm::StackItem`
//! → `neo_vm::StackItem`; `crate::smart_contract::application_engine::*` →
//! `neo_smart_contract::application_engine::*`.

use super::{OracleContract, PendingRequest};
use neo_primitives::UInt160;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::error::{CoreError as Error, CoreResult as Result};
use neo_vm::StackItem;

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
