//
// native_impl.rs - NativeContract trait implementation for ContractManagement
//

use super::*;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::manifest::{ContractEventDescriptor, ContractParameterDefinition};
use crate::smart_contract::storage_context::StorageContext;

fn put_storage_if_changed(
    engine: &mut ApplicationEngine,
    context: &StorageContext,
    key: &[u8],
    value: &[u8],
) -> Result<()> {
    if let Some(existing) = engine.get_storage_item(context, key) {
        if existing == value {
            return Ok(());
        }
    }
    engine.put_storage_item(context, key, value)?;
    Ok(())
}

impl ContractManagement {
    fn parse_hash160_argument(arg: &[u8]) -> Result<UInt160> {
        UInt160::from_bytes(arg).map_err(|e| Error::invalid_argument(format!("Invalid hash: {e}")))
    }
}

impl NativeContract for ContractManagement {
    fn id(&self) -> i32 {
        self.id
    }

    fn initialize(&self, engine: &mut ApplicationEngine) -> Result<()> {
        self.hydrate_from_engine(engine)
    }

    fn name(&self) -> &str {
        "ContractManagement"
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn events(
        &self,
        _settings: &ProtocolSettings,
        _block_height: u32,
    ) -> Vec<ContractEventDescriptor> {
        vec![
            ContractEventDescriptor::new(
                "Deploy".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "Hash".to_string(),
                        ContractParameterType::Hash160,
                    )
                    .expect("Deploy.Hash"),
                ],
            )
            .expect("Deploy event descriptor"),
            ContractEventDescriptor::new(
                "Update".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "Hash".to_string(),
                        ContractParameterType::Hash160,
                    )
                    .expect("Update.Hash"),
                ],
            )
            .expect("Update event descriptor"),
            ContractEventDescriptor::new(
                "Destroy".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "Hash".to_string(),
                        ContractParameterType::Hash160,
                    )
                    .expect("Destroy.Hash"),
                ],
            )
            .expect("Destroy event descriptor"),
        ]
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        match method {
            "getContract" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "getContract requires 1 argument".to_string(),
                    ));
                }
                let hash = Self::parse_hash160_argument(&args[0])?;
                match self.get_contract(&hash)? {
                    Some(contract) => Self::serialize_contract_state(&contract),
                    None => Ok(vec![]),
                }
            }
            "deploy" => {
                if args.len() != 2 && args.len() != 3 {
                    return Err(Error::invalid_argument(
                        "deploy requires 2 or 3 arguments".to_string(),
                    ));
                }
                let nef_bytes = args[0].clone();
                let manifest_bytes = args[1].clone();
                let data = if args.len() == 3 {
                    args[2].clone()
                } else {
                    Vec::new()
                };

                let contract = self.deploy(engine, nef_bytes, manifest_bytes, data)?;
                Self::serialize_contract_state(&contract)
            }
            "update" => {
                if args.len() != 2 && args.len() != 3 {
                    return Err(Error::invalid_argument(
                        "update requires 2 or 3 arguments".to_string(),
                    ));
                }

                let nef_bytes = if args[0].is_empty() {
                    None
                } else {
                    Some(args[0].clone())
                };

                let manifest_bytes = if args[1].is_empty() {
                    None
                } else {
                    Some(args[1].clone())
                };

                let data = if args.len() == 3 {
                    args[2].clone()
                } else {
                    Vec::new()
                };

                self.update(engine, nef_bytes, manifest_bytes, data)?;
                Ok(vec![])
            }
            "destroy" => {
                if !args.is_empty() {
                    return Err(Error::invalid_argument(
                        "destroy requires no arguments".to_string(),
                    ));
                }
                self.destroy(engine)?;
                Ok(vec![])
            }
            "getMinimumDeploymentFee" => {
                if !args.is_empty() {
                    return Err(Error::invalid_argument(
                        "getMinimumDeploymentFee requires no arguments".to_string(),
                    ));
                }
                let fee = self.get_minimum_deployment_fee()?;
                Ok(fee.to_le_bytes().to_vec())
            }
            "setMinimumDeploymentFee" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "setMinimumDeploymentFee requires 1 argument".to_string(),
                    ));
                }
                if args[0].len() != 8 {
                    return Err(Error::invalid_argument("Invalid fee value".to_string()));
                }
                let value = i64::from_le_bytes(
                    args[0]
                        .as_slice()
                        .try_into()
                        .map_err(|_| Error::invalid_argument("Invalid fee value".to_string()))?,
                );
                self.set_minimum_deployment_fee(engine, value)?;
                Ok(vec![])
            }
            "hasMethod" => {
                if args.len() != 3 {
                    return Err(Error::invalid_argument(
                        "hasMethod requires 3 arguments".to_string(),
                    ));
                }
                let hash = Self::parse_hash160_argument(&args[0])?;
                let method = String::from_utf8(args[1].clone()).map_err(|e| {
                    Error::invalid_argument(format!("Invalid method string: {}", e))
                })?;
                if args[2].len() != 4 {
                    return Err(Error::invalid_argument(
                        "Invalid parameter count".to_string(),
                    ));
                }
                let pcount =
                    i32::from_le_bytes(args[2].as_slice().try_into().map_err(|_| {
                        Error::invalid_argument("Invalid parameter count".to_string())
                    })?);
                let result = self.has_method(&hash, &method, pcount)?;
                Ok(vec![if result { 1 } else { 0 }])
            }
            "isContract" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "isContract requires 1 argument".to_string(),
                    ));
                }
                let hash = Self::parse_hash160_argument(&args[0])?;
                let result = Self::is_contract(engine.snapshot_cache().as_ref(), &hash)?;
                Ok(vec![if result { 1 } else { 0 }])
            }
            "getContractById" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "getContractById requires 1 argument".to_string(),
                    ));
                }
                if args[0].len() != 4 {
                    return Err(Error::invalid_argument("Invalid contract ID".to_string()));
                }
                let id = i32::from_le_bytes(
                    args[0]
                        .as_slice()
                        .try_into()
                        .map_err(|_| Error::invalid_argument("Invalid contract ID".to_string()))?,
                );
                match self.get_contract_by_id(id)? {
                    Some(contract) => Self::serialize_contract_state(&contract),
                    None => Ok(vec![]),
                }
            }
            "getContractHashes" => {
                if !args.is_empty() {
                    return Err(Error::invalid_argument(
                        "getContractHashes requires no arguments".to_string(),
                    ));
                }
                let iterator_id = self.get_contract_hashes_iterator(engine)?;
                Ok(iterator_id.to_le_bytes().to_vec())
            }
            _ => Err(Error::native_contract(format!(
                "Method {} not found",
                method
            ))),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn on_persist(&self, engine: &mut ApplicationEngine) -> Result<()> {
        let persisting_block = engine
            .persisting_block()
            .ok_or_else(|| Error::native_contract("No persisting block available"))?;
        let block_height = persisting_block.header.index;
        let settings = engine.protocol_settings().clone();

        let context = engine.get_native_storage_context(&self.hash)?;
        let native_contracts = engine.native_contracts();
        let mut contract_count_changed = false;

        for contract in native_contracts {
            let contract_hash = contract.hash();
            let (should_initialize, _hardforks_now) =
                contract.is_initialize_block(&settings, block_height);
            if !should_initialize {
                continue;
            }

            let Some(mut contract_state) = contract.contract_state(&settings, block_height) else {
                continue;
            };

            let existing_state = {
                let storage = self.storage.read();
                storage.contracts.get(&contract_hash).cloned()
            };

            let is_new = existing_state.is_none();
            if let Some(existing) = existing_state {
                contract_state.update_counter = existing.update_counter.saturating_add(1);
            }

            {
                let mut storage = self.storage.write();
                if is_new {
                    storage
                        .contracts
                        .insert(contract_hash, contract_state.clone());
                    storage
                        .contract_ids
                        .insert(contract_state.id, contract_hash);
                    storage.contract_count = storage.contract_count.saturating_add(1);
                    contract_count_changed = true;
                } else {
                    storage
                        .contracts
                        .insert(contract_hash, contract_state.clone());
                    storage
                        .contract_ids
                        .entry(contract_state.id)
                        .or_insert(contract_hash);
                }
            }

            let contract_bytes = Self::serialize_contract_state(&contract_state)?;
            put_storage_if_changed(
                engine,
                &context,
                &Self::contract_storage_key(&contract_hash),
                &contract_bytes,
            )?;

            let contract_hash_bytes = contract_hash.as_bytes();
            if is_new
                || engine
                    .get_storage_item(&context, &Self::contract_id_storage_key(contract_state.id))
                    .is_none()
            {
                put_storage_if_changed(
                    engine,
                    &context,
                    &Self::contract_id_storage_key(contract_state.id),
                    contract_hash_bytes.as_ref(),
                )?;
            }

            if contract_hash != self.hash {
                contract.initialize(engine)?;
            }

            engine.emit_notification(
                &self.hash,
                if is_new { "Deploy" } else { "Update" },
                &[contract_hash.to_bytes()],
            )?;
        }

        let (min_fee_bytes, next_id_bytes, count_bytes) = {
            let storage = self.storage.read();
            (
                storage.minimum_deployment_fee.to_le_bytes(),
                storage.next_id.to_le_bytes(),
                storage.contract_count.to_le_bytes(),
            )
        };

        if engine
            .get_storage_item(&context, &Self::minimum_deployment_fee_key())
            .is_none()
        {
            put_storage_if_changed(
                engine,
                &context,
                &Self::minimum_deployment_fee_key(),
                &min_fee_bytes,
            )?;
        }

        if engine
            .get_storage_item(&context, &Self::next_id_key())
            .is_none()
        {
            put_storage_if_changed(engine, &context, &Self::next_id_key(), &next_id_bytes)?;
        }

        if contract_count_changed
            || engine
                .get_storage_item(&context, &Self::contract_count_key())
                .is_none()
        {
            put_storage_if_changed(engine, &context, &Self::contract_count_key(), &count_bytes)?;
        }

        Ok(())
    }
}

impl Default for ContractManagement {
    fn default() -> Self {
        Self::new()
    }
}
