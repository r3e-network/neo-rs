use super::OracleContract;
use neo_crypto::Crypto;
use crate::error::{CoreError as Error, CoreResult as Result};
use crate::network::p2p::payloads::transaction_attribute::TransactionAttribute;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::contract::Contract;
use crate::smart_contract::native::{GasToken, Role, RoleManagement};
use crate::UInt160;
use num_bigint::BigInt;
use std::collections::HashMap;

impl OracleContract {
    pub(super) fn cleanup_persisted_responses(
        &self,
        engine: &mut ApplicationEngine,
    ) -> Result<Vec<u64>> {
        let Some(block) = engine.persisting_block().cloned() else {
            return Ok(Vec::new());
        };

        let snapshot_arc = engine.snapshot_cache();
        let snapshot = snapshot_arc.as_ref();
        let mut completed_response_ids = Vec::new();

        for transaction in &block.transactions {
            for attribute in transaction.attributes() {
                if let TransactionAttribute::OracleResponse(response) = attribute {
                    if let Some(request) = self.read_request(snapshot, response.id)? {
                        let url_hash = self.compute_url_hash(&request.url);
                        self.delete_request(snapshot, response.id);
                        self.remove_request_id(snapshot, &url_hash, response.id)?;
                        completed_response_ids.push(response.id);
                    }
                }
            }
        }

        Ok(completed_response_ids)
    }

    pub(super) fn reward_oracle_nodes(
        &self,
        engine: &mut ApplicationEngine,
        response_ids: &[u64],
    ) -> Result<()> {
        if response_ids.is_empty() {
            return Ok(());
        }

        let Some(block) = engine.persisting_block().cloned() else {
            return Ok(());
        };

        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();
        let price = self.get_price_value(snapshot_ref);
        if price <= 0 {
            return Ok(());
        }

        let recipients = self.resolve_oracle_accounts(engine, block.header.index());
        if recipients.is_empty() {
            return Ok(());
        }

        let recipient_count = u64::try_from(recipients.len())
            .map_err(|_| Error::invalid_operation("Too many oracle recipients"))?;
        let mut rewards: HashMap<UInt160, i64> = HashMap::new();
        for response_id in response_ids {
            let index = usize::try_from(*response_id % recipient_count)
                .map_err(|_| Error::invalid_operation("Oracle recipient index overflow"))?;
            let account = recipients[index];
            *rewards.entry(account).or_insert(0) += price;
        }

        if rewards.is_empty() {
            return Ok(());
        }

        let gas = GasToken::new();
        for (account, amount) in rewards {
            if amount <= 0 {
                continue;
            }
            let minted = BigInt::from(amount);
            gas.mint(engine, &account, &minted, false)?;
        }

        Ok(())
    }

    fn resolve_oracle_accounts(&self, engine: &mut ApplicationEngine, index: u32) -> Vec<UInt160> {
        // Use the typed RoleManagement API directly. The previous implementation
        // round-tripped through `call_native_contract` and parsed raw bytes, but
        // `getDesignatedByRole` returns a BinarySerializer-encoded `Array<ByteString(33)>`
        // (see `RoleManagement::serialize_public_keys`), not concatenated 33-byte points.
        // Misparsing left only the first key decodable, funneling all rewards to
        // recipients[0] and breaking the response_id % count distribution at block 754,772.
        let role_mgmt = RoleManagement::new();
        let snapshot_arc = engine.snapshot_cache();
        let snapshot = snapshot_arc.as_ref();
        match role_mgmt.get_designated_by_role_at(snapshot, Role::Oracle, index) {
            Ok(public_keys) => public_keys
                .into_iter()
                .filter_map(|pk| {
                    let script = Contract::create_signature_redeem_script(pk);
                    UInt160::from_bytes(&Crypto::hash160(&script)).ok()
                })
                .collect(),
            Err(err) => {
                tracing::debug!("failed to fetch designated oracle nodes: {}", err);
                Vec::new()
            }
        }
    }
}
