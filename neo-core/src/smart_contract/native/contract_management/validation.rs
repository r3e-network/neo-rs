//
// validation.rs - Validation methods for ContractManagement
//

use super::*;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::manifest::ContractAbi;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::i_interoperable::IInteroperable;
use neo_vm::ExecutionEngineLimits;
use neo_vm::Script;
use std::collections::HashSet;

impl ContractManagement {
    /// Gets the next available contract ID and increments it
    pub(super) fn get_next_available_id(&self) -> Result<i32> {
        let mut storage = self.storage.write();

        let id = storage.next_id;
        storage.next_id += 1;
        Ok(id)
    }

    /// Calculates contract hash using the Neo N3 algorithm (matches C# Helper.GetContractHash).
    pub(super) fn calculate_contract_hash(sender: &UInt160, checksum: u32, name: &str) -> UInt160 {
        Helper::get_contract_hash(sender, checksum, name)
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
        manifest.validate()
    }

    pub(super) fn validate_manifest_groups(
        manifest: &ContractManifest,
        contract_hash: &UInt160,
    ) -> Result<()> {
        for group in &manifest.groups {
            let ok = group.verify_signature(&contract_hash.to_bytes())?;
            if !ok {
                return Err(Error::invalid_data(
                    "Invalid group signature for contract".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn validate_manifest_serialization(
        manifest: &ContractManifest,
        limits: &ExecutionEngineLimits,
    ) -> Result<()> {
        BinarySerializer::serialize(&manifest.to_stack_item(), limits)
            .map_err(|e| Error::invalid_operation(format!("Invalid manifest: {e}")))?;
        Ok(())
    }

    pub(super) fn validate_script_and_abi(script: &Script, abi: &ContractAbi) -> Result<()> {
        let mut seen_methods: HashSet<(String, usize)> = HashSet::new();
        for method in &abi.methods {
            if method.offset < 0 {
                return Err(Error::invalid_data(
                    "Contract method offset cannot be negative".to_string(),
                ));
            }
            let offset = method.offset as usize;
            script
                .get_instruction(offset)
                .map_err(|e| Error::invalid_data(format!("Invalid method offset: {e}")))?;

            let key = (method.name.clone(), method.parameters.len());
            if !seen_methods.insert(key) {
                return Err(Error::invalid_data(
                    "Duplicate contract method definition".to_string(),
                ));
            }
        }

        let mut seen_events: HashSet<String> = HashSet::new();
        for event in &abi.events {
            if !seen_events.insert(event.name.clone()) {
                return Err(Error::invalid_data("Duplicate event name".to_string()));
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
                        if let Ok(hash) = UInt160::from_bytes(&value) {
                            let contract_id = storage.contracts.get(&hash).map(|c| c.id);
                            if let Some(contract_id) = contract_id {
                                storage.contract_ids.entry(contract_id).or_insert(hash);
                            } else {
                                let mut id_bytes = [0u8; 4];
                                id_bytes.copy_from_slice(rest);
                                let id = i32::from_be_bytes(id_bytes);
                                storage.contract_ids.entry(id).or_insert(hash);
                            }
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
