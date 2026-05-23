use super::OracleContract;
use crate::error::CoreResult as Result;
use crate::network::p2p::payloads::transaction_attribute::TransactionAttribute;
use crate::smart_contract::application_engine::ApplicationEngine;

impl OracleContract {
    pub(super) fn verify(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let tx = match engine
            .script_container()
            .and_then(|container| container.as_transaction())
        {
            Some(tx) => tx,
            None => return Ok(vec![0]),
        };
        let snapshot = engine.snapshot_cache();
        let settings = engine.protocol_settings();
        let valid = tx.attributes().iter().any(|attr| match attr {
            TransactionAttribute::OracleResponse(attr) => {
                attr.verify(settings, snapshot.as_ref(), tx)
            }
            _ => false,
        });
        Ok(vec![if valid { 1 } else { 0 }])
    }
}
