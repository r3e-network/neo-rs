//
// query.rs - Query methods for ContractManagement
//

use super::*;
use crate::persistence::{IReadOnlyStoreGeneric, SeekDirection};
use crate::smart_contract::find_options::FindOptions;
use crate::smart_contract::iterators::storage_iterator::StorageIterator;
use crate::smart_contract::StorageItem;
use std::collections::HashMap;

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

        let contract = Self::deserialize_contract_state(&bytes)?;
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

        let contract = Self::deserialize_contract_state(&bytes)?;
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
        let item = store_cache.get(&storage_key).or_else(|| {
            let legacy = StorageKey::new(Self::ID, Self::contract_id_storage_key_legacy(id));
            store_cache.get(&legacy)
        });
        let Some(item) = item else {
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
        if parameter_count < -1 || parameter_count > u16::MAX as i32 {
            return Err(Error::invalid_argument(
                "Parameter count is out of range".to_string(),
            ));
        }
        let storage = self.storage.read();

        if let Some(contract) = storage.contracts.get(hash) {
            let mut abi = contract.manifest.abi.clone();
            Ok(abi.get_method(method, parameter_count).is_some())
        } else {
            Ok(false)
        }
    }

    /// Gets all contract hashes
    pub fn get_contract_hashes(&self) -> Result<Vec<UInt160>> {
        let storage = self.storage.read();
        let mut ids: Vec<_> = storage
            .contract_ids
            .iter()
            .filter_map(|(id, hash)| (*id >= 0).then_some((*id, *hash)))
            .collect();
        ids.sort_by_key(|(id, _)| *id);
        Ok(ids.into_iter().map(|(_, hash)| hash).collect())
    }

    /// Creates an iterator over deployed contract hashes (non-native) matching C# GetContractHashes.
    pub fn get_contract_hashes_iterator(&self, engine: &mut ApplicationEngine) -> Result<u32> {
        let context = engine.get_native_storage_context(&self.hash)?;
        let search_key = StorageKey::new(context.id, vec![PREFIX_CONTRACT_HASH]);

        let mut entries_map: HashMap<StorageKey, StorageItem> = HashMap::new();
        for (key, value) in engine
            .snapshot_cache()
            .find(Some(&search_key), SeekDirection::Forward)
        {
            entries_map.insert(key, value);
        }
        for (key, value) in engine
            .original_snapshot_cache()
            .find(Some(&search_key), SeekDirection::Forward)
        {
            entries_map.entry(key).or_insert(value);
        }

        let mut entries: Vec<(StorageKey, StorageItem)> = entries_map.into_iter().collect();
        entries.sort_by(|a, b| a.0.suffix().cmp(b.0.suffix()));

        let filtered: Vec<(StorageKey, StorageItem)> = entries
            .into_iter()
            .filter(|(key, _)| {
                let suffix = key.suffix();
                if suffix.len() < 1 + 4 {
                    return false;
                }
                let mut id_bytes = [0u8; 4];
                id_bytes.copy_from_slice(&suffix[1..5]);
                i32::from_be_bytes(id_bytes) >= 0
            })
            .collect();

        let iterator = StorageIterator::new(filtered, 1, FindOptions::RemovePrefix);
        let iterator_id = engine
            .store_storage_iterator(iterator)
            .map_err(Error::native_contract)?;
        Ok(iterator_id)
    }

    /// Checks whether a contract exists in the provided snapshot cache.
    pub fn is_contract(snapshot: &DataCache, hash: &UInt160) -> Result<bool> {
        let key = StorageKey::new(Self::ID, Self::contract_storage_key(hash));
        Ok(snapshot.contains(&key))
    }

    /// Lists all non-native contracts from the snapshot cache.
    pub fn list_contracts(snapshot: &DataCache) -> Result<Vec<ContractState>> {
        let prefix = StorageKey::new(Self::ID, vec![PREFIX_CONTRACT]);
        let mut contracts: Vec<ContractState> = snapshot
            .find(Some(&prefix), SeekDirection::Forward)
            .filter_map(|(_, item)| {
                let bytes = item.get_value();
                Self::deserialize_contract_state(&bytes).ok()
            })
            .filter(|contract| contract.id >= 0)
            .collect();
        contracts.sort_by_key(|contract| contract.id);
        Ok(contracts)
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
