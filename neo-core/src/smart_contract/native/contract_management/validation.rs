//
// validation.rs - Validation methods for ContractManagement
//

use super::*;

impl ContractManagement {
    /// Gets the next available contract ID and increments it
    pub(super) fn get_next_available_id(&self) -> Result<i32> {
        let mut storage = self.storage.write();

        let id = storage.next_id;
        storage.next_id += 1;
        Ok(id)
    }

    /// Calculates contract hash from script
    ///
    /// # Panics
    /// This function will not panic as SHA256 always produces 32 bytes,
    /// and UInt160 requires only 20 bytes which is always available.
    pub(super) fn calculate_contract_hash(sender: &UInt160, checksum: u32, name: &str) -> UInt160 {
        let mut buffer = Vec::with_capacity(1 + sender.as_bytes().len() + 4 + name.len());
        buffer.push(0xFF); // Contract prefix
        buffer.extend_from_slice(&sender.as_bytes());
        buffer.extend_from_slice(&checksum.to_le_bytes());
        buffer.extend_from_slice(name.as_bytes());

        let hash = Crypto::sha256(&buffer);
        // SAFETY: SHA256 always produces 32 bytes, we only need 20
        UInt160::from_bytes(&hash[..20])
            .unwrap_or_else(|_| unreachable!("SHA256 output is always >= 20 bytes"))
    }

    /// Validates NEF file structure
    pub(super) fn validate_nef_file(nef: &NefFile) -> Result<()> {
        let max_item_size = ExecutionEngineLimits::default().max_item_size as usize;

        if nef.script.is_empty() {
            return Err(Error::invalid_data("Script cannot be empty".to_string()));
        }

        if nef.script.len() > max_item_size {
            return Err(Error::invalid_data(format!(
                "Script size {} exceeds MaxItemSize {}",
                nef.script.len(),
                max_item_size
            )));
        }

        // Validate checksum matches C# NEF algorithm.
        let mut cloned = nef.clone();
        cloned.update_checksum();
        if cloned.checksum != nef.checksum {
            return Err(Error::invalid_data("Invalid NEF checksum".to_string()));
        }

        Ok(())
    }

    /// Validates contract manifest
    pub(super) fn validate_manifest(manifest: &ContractManifest) -> Result<()> {
        // Validate ABI
        if manifest.abi.methods.is_empty() {
            return Err(Error::invalid_data(
                "Contract must have at least one method".to_string(),
            ));
        }

        // Validate permissions
        for permission in &manifest.permissions {
            // Check if permission is valid - at least one must be specified
            let contract_valid = match &permission.contract {
                ContractPermissionDescriptor::Wildcard => true,
                ContractPermissionDescriptor::Hash(_) => true,
                ContractPermissionDescriptor::Group(_) => true,
            };

            if !contract_valid {
                return Err(Error::invalid_data(
                    "Invalid permission definition".to_string(),
                ));
            }
        }

        // Validate groups
        for group in &manifest.groups {
            // ECPoint always has a value, check signature
            if group.signature.is_empty() {
                return Err(Error::invalid_data(
                    "Invalid group definition - missing signature".to_string(),
                ));
            }
        }

        Ok(())
    }

    pub(super) fn hydrate_from_engine(&self, engine: &ApplicationEngine) -> Result<()> {
        let entries = engine.storage_entries_for_contract(&self.hash);
        if entries.is_empty() {
            return Ok(());
        }

        let mut storage = self.storage.write();

        storage.contracts.clear();
        storage.contract_ids.clear();
        storage.contract_count = 0;
        storage.next_id = 1;
        storage.minimum_deployment_fee = DEFAULT_MINIMUM_DEPLOYMENT_FEE;

        for (key, item) in entries {
            let key_bytes = key.suffix();
            if key_bytes.is_empty() {
                continue;
            }

            // SAFETY: We already checked key_bytes is not empty above
            let Some((prefix, rest)) = key_bytes.split_first() else {
                continue;
            };
            let value = item.get_value();
            match *prefix {
                PREFIX_CONTRACT => {
                    if let Ok(contract_hash) = UInt160::from_bytes(rest) {
                        let mut reader = MemoryReader::new(&value);
                        if let Ok(contract_state) = ContractState::deserialize(&mut reader) {
                            storage
                                .contract_ids
                                .insert(contract_state.id, contract_hash);
                            storage.contracts.insert(contract_hash, contract_state);
                        }
                    }
                }
                PREFIX_CONTRACT_HASH => {
                    if rest.len() == 4 {
                        let mut id_bytes = [0u8; 4];
                        id_bytes.copy_from_slice(rest);
                        let id = i32::from_le_bytes(id_bytes);
                        if let Ok(hash) = UInt160::from_bytes(&value) {
                            storage.contract_ids.insert(id, hash);
                        }
                    }
                }
                PREFIX_CONTRACT_COUNT => {
                    if value.len() == 4 {
                        let mut buf = [0u8; 4];
                        buf.copy_from_slice(&value[..4]);
                        storage.contract_count = u32::from_le_bytes(buf);
                    }
                }
                PREFIX_NEXT_AVAILABLE_ID => {
                    if value.len() == 4 {
                        let mut buf = [0u8; 4];
                        buf.copy_from_slice(&value[..4]);
                        storage.next_id = i32::from_le_bytes(buf);
                    }
                }
                PREFIX_MINIMUM_DEPLOYMENT_FEE => {
                    if value.len() == 8 {
                        let mut buf = [0u8; 8];
                        buf.copy_from_slice(&value[..8]);
                        storage.minimum_deployment_fee = i64::from_le_bytes(buf);
                    }
                }
                _ => {}
            }
        }

        if storage.contract_count == 0 {
            storage.contract_count = storage.contracts.len() as u32;
        }

        if let Some(max_id) = storage.contract_ids.keys().copied().max() {
            if storage.next_id <= max_id {
                storage.next_id = max_id + 1;
            }
        }

        Ok(())
    }
}
