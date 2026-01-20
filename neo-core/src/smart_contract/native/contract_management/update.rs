//
// update.rs - Contract update for ContractManagement
//

use super::*;
use crate::hardfork::Hardfork;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::manifest::contract_manifest::MAX_MANIFEST_LENGTH;
use neo_vm::Script;

impl ContractManagement {
    /// Updates an existing contract
    pub fn update(
        &self,
        engine: &mut ApplicationEngine,
        nef_file: Option<Vec<u8>>,
        manifest_bytes: Option<Vec<u8>>,
        data: Vec<u8>,
    ) -> Result<()> {
        if nef_file.is_none() && manifest_bytes.is_none() {
            return Err(Error::invalid_argument(
                "NEF file and manifest cannot both be empty".to_string(),
            ));
        }

        if let Some(nef_bytes) = nef_file.as_ref() {
            if nef_bytes.is_empty() {
                return Err(Error::invalid_argument(
                    "NEF file length cannot be zero".to_string(),
                ));
            }
        }
        if let Some(manifest_payload) = manifest_bytes.as_ref() {
            if manifest_payload.is_empty() {
                return Err(Error::invalid_argument(
                    "Manifest length cannot be zero".to_string(),
                ));
            }
            if manifest_payload.len() > MAX_MANIFEST_LENGTH {
                return Err(Error::invalid_argument(
                    "Manifest exceeds maximum allowed length".to_string(),
                ));
            }
        }

        if engine.is_hardfork_enabled(Hardfork::HfAspidochelone) {
            if let Ok(state) = engine.current_execution_state() {
                let state = state.lock();
                if !state.call_flags.contains(CallFlags::ALL) {
                    return Err(Error::invalid_operation(format!(
                        "Cannot call Update with the flag {:?}.",
                        state.call_flags
                    )));
                }
            }
        }

        // Get calling contract hash
        let contract_hash = engine
            .get_calling_script_hash()
            .ok_or_else(|| Error::invalid_operation("No calling context".to_string()))?;

        // Get existing contract
        let mut contract = {
            let storage = self.storage.read();

            storage
                .contracts
                .get(&contract_hash)
                .cloned()
                .ok_or_else(|| Error::invalid_operation("Contract not found".to_string()))?
        };

        if contract.update_counter == u16::MAX {
            return Err(Error::invalid_operation(
                "The contract reached the maximum number of updates.".to_string(),
            ));
        }

        let nef_len = nef_file.as_ref().map(|v| v.len()).unwrap_or(0);
        let manifest_len = manifest_bytes.as_ref().map(|v| v.len()).unwrap_or(0);
        let payload_size = nef_len.saturating_add(manifest_len);
        if payload_size > 0 {
            let storage_fee = (engine.storage_price() as u64).saturating_mul(payload_size as u64);
            if storage_fee > 0 {
                engine.charge_execution_fee(storage_fee)?;
            }
        }

        // Update NEF if provided
        if let Some(nef_bytes) = nef_file {
            let mut reader = MemoryReader::new(&nef_bytes);
            let nef = <NefFile as crate::neo_io::Serializable>::deserialize(&mut reader)
                .map_err(|e| Error::deserialization(format!("Invalid NEF file: {}", e)))?;

            Self::validate_nef_file(&nef)?;
            contract.nef = nef;
        }

        // Clean whitelist entries using the old manifest before updates.
        PolicyContract::new().clean_whitelist(engine, &contract)?;

        // Update manifest if provided
        if let Some(manifest_payload) = manifest_bytes {
            let manifest_json = String::from_utf8(manifest_payload)
                .map_err(|e| Error::deserialization(format!("Invalid manifest encoding: {}", e)))?;
            let manifest: ContractManifest = serde_json::from_str(&manifest_json)
                .map_err(|e| Error::deserialization(format!("Invalid manifest: {}", e)))?;

            if manifest.name != contract.manifest.name {
                return Err(Error::invalid_operation(
                    "The name of the contract cannot be changed".to_string(),
                ));
            }
            Self::validate_manifest(&manifest)?;
            Self::validate_manifest_groups(&manifest, &contract_hash)?;
            Self::validate_manifest_serialization(&manifest, engine.execution_limits())?;
            contract.manifest = manifest;
        }

        let strict = engine.is_hardfork_enabled(Hardfork::HfBasilisk);
        let script = Script::new(contract.nef.script.clone(), strict)
            .map_err(|e| Error::invalid_data(format!("Invalid contract script: {e}")))?;
        Self::validate_script_and_abi(&script, &contract.manifest.abi)?;

        // Increment update counter
        contract.update_counter += 1;

        // Update storage
        let contract_bytes = Self::serialize_contract_state(&contract)?;

        {
            let mut storage = self.storage.write();

            storage.contracts.insert(contract_hash, contract.clone());
        }

        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(
            &context,
            &Self::contract_storage_key(&contract_hash),
            &contract_bytes,
        )?;

        // Call contract's _update method if it exists
        if contract
            .manifest
            .abi
            .methods
            .iter()
            .any(|m| m.name == "_deploy")
        {
            self.invoke_deploy_hook(engine, &contract_hash, &data, true)?;
        }

        // Emit Update event
        engine.emit_notification(&self.hash, "Update", &[contract_hash.to_bytes()])?;

        Ok(())
    }
}
