//
// query.rs - Query methods for ContractManagement
//

use super::*;

impl ContractManagement {
    /// Gets a contract by hash
    pub fn get_contract(&self, hash: &UInt160) -> Result<Option<ContractState>> {
        let storage = self.storage.read();

        Ok(storage.contracts.get(hash).cloned())
    }

    /// Gets a contract by hash using the provided snapshot cache (matches C# snapshot lookup).
    pub fn get_contract_from_snapshot(
        snapshot: &DataCache,
        hash: &UInt160,
    ) -> Result<Option<ContractState>> {
        let storage_key = StorageKey::new(Self::ID, Self::contract_storage_key(hash));
        let Some(item) = snapshot.get(&storage_key) else {
            return Ok(None);
        };

        let bytes = item.get_value();
        if bytes.is_empty() {
            return Ok(None);
        }

        let mut reader = MemoryReader::new(&bytes);
        let contract = <ContractState as Serializable>::deserialize(&mut reader).map_err(|e| {
            Error::deserialization(format!("Failed to deserialize contract: {}", e))
        })?;
        Ok(Some(contract))
    }

    /// Gets the contract from the provided store cache (including persisted storage).
    pub fn get_contract_from_store_cache(
        store_cache: &StoreCache,
        hash: &UInt160,
    ) -> Result<Option<ContractState>> {
        let storage_key = StorageKey::new(Self::ID, Self::contract_storage_key(hash));
        let Some(item) = store_cache.get(&storage_key) else {
            return Ok(None);
        };

        let bytes = item.get_value();
        if bytes.is_empty() {
            return Ok(None);
        }

        let mut reader = MemoryReader::new(&bytes);
        let contract = <ContractState as Serializable>::deserialize(&mut reader).map_err(|e| {
            Error::deserialization(format!("Failed to deserialize contract: {}", e))
        })?;
        Ok(Some(contract))
    }

    /// Gets a contract by ID from the provided store cache.
    pub fn get_contract_by_id_from_store_cache(
        store_cache: &StoreCache,
        id: i32,
    ) -> Result<Option<ContractState>> {
        let hash = match Self::get_contract_hash_by_id_from_store_cache(store_cache, id)? {
            Some(hash) => hash,
            None => return Ok(None),
        };
        Self::get_contract_from_store_cache(store_cache, &hash)
    }

    /// Gets a contract by ID
    pub fn get_contract_by_id(&self, id: i32) -> Result<Option<ContractState>> {
        let storage = self.storage.read();

        if let Some(hash) = storage.contract_ids.get(&id) {
            Ok(storage.contracts.get(hash).cloned())
        } else {
            Ok(None)
        }
    }

    fn get_contract_hash_by_id_from_store_cache(
        store_cache: &StoreCache,
        id: i32,
    ) -> Result<Option<UInt160>> {
        let storage_key = StorageKey::new(Self::ID, Self::contract_id_storage_key(id));
        let Some(item) = store_cache.get(&storage_key) else {
            return Ok(None);
        };

        let bytes = item.get_value();
        if bytes.is_empty() {
            return Ok(None);
        }

        let hash = UInt160::from_bytes(&bytes)
            .map_err(|e| Error::invalid_data(format!("Invalid contract hash bytes: {e}")))?;
        Ok(Some(hash))
    }

    /// Checks if a contract has a specific method
    pub fn has_method(&self, hash: &UInt160, method: &str, parameter_count: i32) -> Result<bool> {
        let storage = self.storage.read();

        if let Some(contract) = storage.contracts.get(hash) {
            Ok(contract
                .manifest
                .abi
                .methods
                .iter()
                .any(|m| m.name == method && m.parameters.len() == parameter_count as usize))
        } else {
            Ok(false)
        }
    }

    /// Gets all contract hashes
    pub fn get_contract_hashes(&self) -> Result<Vec<UInt160>> {
        let storage = self.storage.read();

        Ok(storage.contracts.keys().cloned().collect())
    }

    /// Gets the minimum deployment fee
    pub fn get_minimum_deployment_fee(&self) -> Result<i64> {
        let storage = self.storage.read();

        Ok(storage.minimum_deployment_fee)
    }

    /// Sets the minimum deployment fee (committee only)
    pub fn set_minimum_deployment_fee(
        &self,
        engine: &mut ApplicationEngine,
        value: i64,
    ) -> Result<()> {
        if value < 0 {
            return Err(Error::invalid_argument(
                "Deployment fee cannot be negative".to_string(),
            ));
        }

        // Check committee permission
        if !engine.check_committee_witness()? {
            return Err(Error::invalid_operation(
                "Committee witness required".to_string(),
            ));
        }

        // Update storage
        let (min_fee_bytes, next_id_bytes) = {
            let mut storage = self.storage.write();

            storage.minimum_deployment_fee = value;

            (
                storage.minimum_deployment_fee.to_le_bytes(),
                storage.next_id.to_le_bytes(),
            )
        };

        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(
            &context,
            &Self::minimum_deployment_fee_key(),
            &min_fee_bytes,
        )?;
        engine.put_storage_item(&context, &Self::next_id_key(), &next_id_bytes)?;

        Ok(())
    }
}
