//
// deploy.rs - Contract deployment for ContractManagement
//

use super::*;
use crate::hardfork::Hardfork;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::manifest::contract_manifest::MAX_MANIFEST_LENGTH;
use crate::smart_contract::native::security_fixes::{
    PermissionValidator, ReentrancyGuardType, SafeArithmetic, SecurityContext,
};
use neo_vm::Script;

impl ContractManagement {
    /// Deploys a new contract
    pub fn deploy(
        &self,
        engine: &mut ApplicationEngine,
        nef_file: Vec<u8>,
        manifest_bytes: Vec<u8>,
        data: Vec<u8>,
    ) -> Result<ContractState> {
        // Enter reentrancy guard
        let _guard = SecurityContext::enter_guard(ReentrancyGuardType::ContractDeploy)?;

        if nef_file.is_empty() {
            return Err(Error::invalid_argument(
                "NEF file length cannot be zero".to_string(),
            ));
        }
        if manifest_bytes.is_empty() {
            return Err(Error::invalid_argument(
                "Manifest length cannot be zero".to_string(),
            ));
        }
        if manifest_bytes.len() > MAX_MANIFEST_LENGTH {
            return Err(Error::invalid_argument(
                "Manifest exceeds maximum allowed length".to_string(),
            ));
        }

        // Validate payload sizes don't overflow
        let nef_len = nef_file.len();
        let manifest_len = manifest_bytes.len();
        SafeArithmetic::check_add_overflow(nef_len, manifest_len)
            .map_err(|e| Error::invalid_argument(format!("Payload size overflow: {}", e)))?;

        if engine.is_hardfork_enabled(Hardfork::HfAspidochelone) {
            if let Ok(state) = engine.current_execution_state() {
                let state = state.lock();
                if !state.call_flags.contains(CallFlags::ALL) {
                    return Err(Error::invalid_operation(format!(
                        "Cannot call Deploy with the flag {:?}.",
                        state.call_flags
                    )));
                }
            }
        }

        // Parse and validate NEF file
        let mut reader = MemoryReader::new(&nef_file);
        let nef = <NefFile as crate::neo_io::Serializable>::deserialize(&mut reader)
            .map_err(|e| Error::deserialization(format!("Invalid NEF file: {}", e)))?;

        Self::validate_nef_file(&nef)?;

        let manifest_size = manifest_bytes.len();
        // Parse and validate manifest
        let manifest_json = String::from_utf8(manifest_bytes)
            .map_err(|e| Error::deserialization(format!("Invalid manifest encoding: {}", e)))?;
        let manifest: ContractManifest = serde_json::from_str(&manifest_json)
            .map_err(|e| Error::deserialization(format!("Invalid manifest: {}", e)))?;

        Self::validate_manifest(&manifest)?;

        // Require transaction sender (matches C# behaviour)
        let sender = engine
            .get_transaction_sender()
            .ok_or_else(|| Error::invalid_operation("Deploy must be invoked by a transaction"))?;

        // Calculate contract hash
        let contract_hash = Self::calculate_contract_hash(&sender, nef.checksum, &manifest.name);
        Self::validate_manifest_groups(&manifest, &contract_hash)?;
        Self::validate_manifest_serialization(&manifest, engine.execution_limits())?;

        let strict = engine.is_hardfork_enabled(Hardfork::HfBasilisk);
        let script = Script::new(nef.script.clone(), strict)
            .map_err(|e| Error::invalid_data(format!("Invalid contract script: {e}")))?;
        Self::validate_script_and_abi(&script, &manifest.abi)?;

        // Ensure hash is not blocked by policy
        let policy = PolicyContract::new();
        let snapshot = engine.snapshot_cache();
        if policy
            .is_blocked_snapshot(snapshot.as_ref(), &contract_hash)
            .map_err(|e| Error::native_contract(e.to_string()))?
        {
            return Err(Error::invalid_operation(
                "Contract hash is blocked".to_string(),
            ));
        }

        // Check if contract already exists
        {
            let storage = self.storage.read();

            if storage.contracts.contains_key(&contract_hash) {
                return Err(Error::invalid_operation(
                    "Contract already exists".to_string(),
                ));
            }
        }

        // Check deployment fee
        let deployment_fee = {
            let storage = self.storage.read();
            storage.minimum_deployment_fee
        };

        // Deduct the larger of storage fee and minimum deployment fee
        let payload_size = nef_file.len().saturating_add(manifest_size);

        // Use safe arithmetic for fee calculation
        let storage_fee =
            SafeArithmetic::check_mul_overflow(engine.storage_price() as u64, payload_size as u64)
                .map_err(|e| {
                    Error::invalid_argument(format!("Storage fee calculation overflow: {}", e))
                })?;

        let minimum_fee = u64::try_from(deployment_fee.max(0)).unwrap_or(0);
        let fee_to_charge = storage_fee.max(minimum_fee);

        // Validate fee is reasonable
        PermissionValidator::validate_range(fee_to_charge, 0, u64::MAX / 2, "Deployment fee")
            .map_err(|e| Error::invalid_argument(format!("Invalid deployment fee: {}", e)))?;

        if fee_to_charge > 0 {
            engine.charge_execution_fee(fee_to_charge)?;
        }

        // Get next contract ID
        let contract_id = self.get_next_available_id()?;

        // Create contract state
        let contract = ContractState::new(contract_id, contract_hash, nef, manifest);

        // Serialize contract state for persistence
        let contract_bytes = Self::serialize_contract_state(&contract)?;
        let contract_hash_bytes = contract_hash.as_bytes();

        // Store contract in in-memory cache and prepare metadata snapshots
        let (contract_count_bytes, next_id_bytes, min_fee_bytes) = {
            let mut storage = self.storage.write();

            storage.contracts.insert(contract_hash, contract.clone());
            storage.contract_ids.insert(contract_id, contract_hash);
            storage.contract_count += 1;

            (
                storage.contract_count.to_le_bytes(),
                storage.next_id.to_le_bytes(),
                storage.minimum_deployment_fee.to_le_bytes(),
            )
        };

        // Persist contract metadata in native storage so it survives engine reloads
        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(
            &context,
            &Self::contract_storage_key(&contract_hash),
            &contract_bytes,
        )?;
        engine.put_storage_item(
            &context,
            &Self::contract_id_storage_key(contract_id),
            contract_hash_bytes.as_ref(),
        )?;
        engine.put_storage_item(&context, &Self::contract_count_key(), &contract_count_bytes)?;
        engine.put_storage_item(&context, &Self::next_id_key(), &next_id_bytes)?;
        engine.put_storage_item(
            &context,
            &Self::minimum_deployment_fee_key(),
            &min_fee_bytes,
        )?;

        // Call contract's _deploy method if it exists
        if contract
            .manifest
            .abi
            .methods
            .iter()
            .any(|m| m.name == "_deploy")
        {
            self.invoke_deploy_hook(engine, &contract_hash, &data, false)?;
        }

        // Emit Deploy event
        engine.emit_notification(&self.hash, "Deploy", &[contract_hash.to_bytes()])?;

        Ok(contract)
    }
}
