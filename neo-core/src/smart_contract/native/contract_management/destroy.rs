//
// destroy.rs - Contract destruction for ContractManagement
//

use super::*;

impl ContractManagement {
    /// Destroys a contract
    pub fn destroy(&self, engine: &mut ApplicationEngine) -> Result<()> {
        // Get calling contract hash
        let contract_hash = engine
            .get_calling_script_hash()
            .ok_or_else(|| Error::invalid_operation("No calling context"))?;

        // Get contract to destroy
        let contract = {
            let storage = self.storage.read();

            storage
                .contracts
                .get(&contract_hash)
                .cloned()
                .ok_or_else(|| Error::invalid_operation("Contract not found"))?
        };

        // Update in-memory cache and prepare metadata snapshots
        let (next_id_bytes, min_fee_bytes) = {
            let mut storage = self.storage.write();

            storage.contracts.remove(&contract_hash);
            storage.contract_ids.remove(&contract.id);
            storage.contract_count = storage.contract_count.saturating_sub(1);

            (
                Self::encode_storage_i32(storage.next_id),
                Self::encode_storage_i64(storage.minimum_deployment_fee),
            )
        };

        // NOTE: contract_count is NOT persisted — C# derives it at runtime.
        let context = engine.get_native_storage_context(&self.hash)?;
        engine.delete_storage_item(&context, &Self::contract_storage_key(&contract_hash))?;
        engine.delete_storage_item(&context, &Self::contract_id_storage_key(contract.id))?;
        engine.delete_storage_item(&context, &Self::contract_id_storage_key_legacy(contract.id))?;
        engine.put_storage_item(&context, &Self::next_id_key(), &next_id_bytes)?;
        engine.put_storage_item(
            &context,
            &Self::minimum_deployment_fee_key(),
            &min_fee_bytes,
        )?;

        // Clear all contract storage (would interact with persistence layer)
        engine.clear_contract_storage(&contract_hash)?;

        // Block the contract hash so it cannot be redeployed without governance approval
        let _ = PolicyContract::new().block_account_internal(engine, &contract_hash)?;

        // Clean whitelist entries (emit events) for the destroyed contract.
        PolicyContract::new().clean_whitelist(engine, &contract)?;

        // Emit Destroy event
        engine.emit_notification(&self.hash, "Destroy", &[contract_hash.to_bytes()])?;

        Ok(())
    }
}
