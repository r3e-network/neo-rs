use std::collections::HashMap;

#[contract]
pub struct ContractManagement {
    storage_map: StorageMap,
}

const PREFIX_MINIMUM_DEPLOYMENT_FEE: u8 = 20;
const PREFIX_NEXT_AVAILABLE_ID: u8 = 15;
const PREFIX_CONTRACT: u8 = 8;
const PREFIX_CONTRACT_HASH: u8 = 12;

#[event]
pub enum Event {
    #[event("Deploy")]
    Deploy(Hash160),
    #[event("Update")]
    Update(Hash160),
    #[event("Destroy")]
    Destroy(Hash160),
}

impl ContractManagement {
    fn new() -> Self {
        Self {
            storage_map: StorageMap::new(Storage::get_context()),
        }
    }

    fn get_next_available_id(&mut self) -> u32 {
        let key = Self::create_storage_key(PREFIX_NEXT_AVAILABLE_ID);
        let mut value = self.storage_map.get(&key).unwrap_or(0u32);
        value += 1;
        self.storage_map.put(&key, &value);
        value
    }

    #[initialize]
    fn initialize(&mut self, hardfork: Option<Hardfork>) {
        if hardfork == Some(self.active_in()) {
            self.storage_map.put(&Self::create_storage_key(PREFIX_MINIMUM_DEPLOYMENT_FEE), &1_000_000_000u64);
            self.storage_map.put(&Self::create_storage_key(PREFIX_NEXT_AVAILABLE_ID), &1u32);
        }
    }

    async fn on_deploy(&self, contract: &Contract, data: StackItem, update: bool) -> Result<(), String> {
        let method = if update { "update" } else { "deploy" };
        if let Some(md) = contract.manifest.abi.get_method(method) {
            Runtime::call_contract(contract.hash, md.name, &[data])?;
        }
        let event = if update { Event::Update } else { Event::Deploy };
        Runtime::notify(&event(contract.hash));
        Ok(())
    }

    #[on_persist]
    async fn on_persist(&mut self) -> Result<(), String> {
        for contract in Contract::native_contracts() {
            if let Some(hfs) = contract.is_initialize_block(Runtime::get_block()) {
                let contract_state = contract.get_contract_state();
                let key = Self::create_storage_key(PREFIX_CONTRACT).extend(&contract.hash);
                let state = self.storage_map.get(&key);

                if state.is_none() {
                    self.storage_map.put(&key, &contract_state);
                    let hash_key = Self::create_storage_key(PREFIX_CONTRACT_HASH).extend(&contract.id.to_be_bytes());
                    self.storage_map.put(&hash_key, &contract.hash);

                    if contract.active_in.is_none() {
                        contract.initialize(None)?;
                    }
                } else {
                    let mut old_contract: Contract = state.unwrap();
                    old_contract.update_counter += 1;
                    old_contract.nef = contract_state.nef;
                    old_contract.manifest = contract_state.manifest;
                    self.storage_map.put(&key, &old_contract);
                }

                if let Some(hfs) = hfs {
                    for hf in hfs {
                        contract.initialize(Some(hf))?;
                    }
                }

                let event = if state.is_none() { Event::Deploy } else { Event::Update };
                Runtime::notify(&event(contract.hash));
            }
        }
        Ok(())
    }

    #[contract_method(cpu_fee = 32768, required_flags = "read_states")]
    fn get_minimum_deployment_fee(&self) -> u64 {
        self.storage_map.get(&Self::create_storage_key(PREFIX_MINIMUM_DEPLOYMENT_FEE)).unwrap_or(0)
    }

    #[contract_method(cpu_fee = 32768, required_flags = "states")]
    fn set_minimum_deployment_fee(&mut self, value: u64) -> Result<(), String> {
        if !Runtime::check_witness(&Runtime::executing_script_hash()) {
            return Err("No authorization".into());
        }
        self.storage_map.put(&Self::create_storage_key(PREFIX_MINIMUM_DEPLOYMENT_FEE), &value);
        Ok(())
    }

    #[contract_method(cpu_fee = 32768, required_flags = "read_states")]
    pub fn get_contract(&self, hash: &Hash160) -> Option<Contract> {
        let key = Self::create_storage_key(PREFIX_CONTRACT).extend(hash);
        self.storage_map.get(&key)
    }

    #[contract_method(cpu_fee = 32768, required_flags = "read_states")]
    pub fn get_contract_by_id(&self, id: u32) -> Option<Contract> {
        let key = Self::create_storage_key(PREFIX_CONTRACT_HASH).extend(&id.to_be_bytes());
        if let Some(hash) = self.storage_map.get::<Hash160>(&key) {
            self.get_contract(&hash)
        } else {
            None
        }
    }

    #[contract_method(cpu_fee = 32768, required_flags = "read_states")]
    fn get_contract_hashes(&self) -> Vec<Hash160> {
        self.storage_map
            .find(Self::create_storage_key(PREFIX_CONTRACT_HASH).as_slice())
            .filter_map(|(_, v)| v.try_into().ok())
            .collect()
    }

    #[contract_method(cpu_fee = 32768, required_flags = "read_states")]
    pub fn has_method(&self, hash: &Hash160, method: &str, param_count: usize) -> bool {
        if let Some(contract) = self.get_contract(hash) {
            contract.manifest.abi.get_method(method)
                .map_or(false, |m| m.parameters.len() == param_count)
        } else {
            false
        }
    }

    pub fn list_contracts(&self) -> Vec<Contract> {
        self.storage_map
            .find(Self::create_storage_key(PREFIX_CONTRACT).as_slice())
            .filter_map(|(_, v)| v.try_into().ok())
            .collect()
    }

    #[contract_method(required_flags = "all")]
    async fn deploy(&mut self, nef_file: Vec<u8>, manifest: Vec<u8>, data: StackItem) -> Result<Contract, String> {
        let tx = Runtime::get_transaction().ok_or("Not in a transaction context")?;
        if nef_file.is_empty() || manifest.is_empty() {
            return Err("Invalid NEF or manifest".into());
        }

        let storage_fee = Runtime::get_storage_price() * ((nef_file.len() + manifest.len()) as u64);
        let min_fee = self.get_minimum_deployment_fee();
        Runtime::add_fee(std::cmp::max(storage_fee, min_fee));

        let nef = NefFile::deserialize(&nef_file)?;
        let parsed_manifest = ContractManifest::parse(&manifest)?;
        Helper::check(&nef.script, &parsed_manifest.abi)?;

        let hash = Helper::get_contract_hash(&tx.sender, nef.check_sum, &parsed_manifest.name);

        if Policy::is_blocked(&hash) {
            return Err(format!("The contract {} has been blocked", hash));
        }

        let key = Self::create_storage_key(PREFIX_CONTRACT).extend(&hash);
        if self.storage_map.get::<Contract>(&key).is_some() {
            return Err(format!("Contract Already Exists: {}", hash));
        }

        let contract = Contract {
            id: self.get_next_available_id(),
            update_counter: 0,
            nef,
            hash,
            manifest: parsed_manifest,
        };

        if !contract.manifest.is_valid(&Runtime::get_limits(), &hash) {
            return Err(format!("Invalid Manifest: {}", hash));
        }

        self.storage_map.put(&key, &contract);
        let hash_key = Self::create_storage_key(PREFIX_CONTRACT_HASH).extend(&contract.id.to_be_bytes());
        self.storage_map.put(&hash_key, &hash);

        self.on_deploy(&contract, data, false).await?;

        Ok(contract)
    }

    #[contract_method(required_flags = "all")]
    async fn update(&mut self, nef_file: Option<Vec<u8>>, manifest: Option<Vec<u8>>, data: StackItem) -> Result<(), String> {
        if nef_file.is_none() && manifest.is_none() {
            return Err("Either NEF or manifest must be provided".into());
        }

        let storage_fee = Runtime::get_storage_price() * ((nef_file.as_ref().map_or(0, |n| n.len()) + manifest.as_ref().map_or(0, |m| m.len())) as u64);
        Runtime::add_fee(storage_fee);

        let key = Self::create_storage_key(PREFIX_CONTRACT).extend(&Runtime::calling_script_hash());
        let mut contract: Contract = self.storage_map.get(&key).ok_or("Updating Contract Does Not Exist")?;

        if contract.update_counter == u16::MAX {
            return Err("The contract reached the maximum number of updates".into());
        }

        if let Some(nef) = nef_file {
            if nef.is_empty() {
                return Err("Invalid NefFile Length".into());
            }
            contract.nef = NefFile::deserialize(&nef)?;
        }

        if let Some(new_manifest) = manifest {
            if new_manifest.is_empty() {
                return Err("Invalid Manifest Length".into());
            }
            let manifest_new = ContractManifest::parse(&new_manifest)?;
            if manifest_new.name != contract.manifest.name {
                return Err("The name of the contract can't be changed".into());
            }
            if !manifest_new.is_valid(&Runtime::get_limits(), &contract.hash) {
                return Err(format!("Invalid Manifest: {}", contract.hash));
            }
            contract.manifest = manifest_new;
        }

        Helper::check(&contract.nef.script, &contract.manifest.abi)?;
        contract.update_counter += 1;

        self.storage_map.put(&key, &contract);
        self.on_deploy(&contract, data, true).await
    }

    #[contract_method(cpu_fee = 32768, required_flags = "states | allow_notify")]
    fn destroy(&mut self) {
        let hash = Runtime::calling_script_hash();
        let key = Self::create_storage_key(PREFIX_CONTRACT).extend(&hash);
        if let Some(contract) = self.storage_map.get::<Contract>(&key) {
            self.storage_map.delete(&key);
            let hash_key = Self::create_storage_key(PREFIX_CONTRACT_HASH).extend(&contract.id.to_be_bytes());
            self.storage_map.delete(&hash_key);
            
            // Delete all contract storage
            let prefix = StorageKey::create_search_prefix(contract.id, &[]);
            for (key, _) in self.storage_map.find(prefix.as_slice()) {
                self.storage_map.delete(&key);
            }

            // Lock contract
            Policy::block_account(&hash);

            // Emit event
            Runtime::notify(&Event::Destroy(hash));
        }
    }

    fn create_storage_key(prefix: u8) -> Vec<u8> {
        vec![prefix]
    }
}
