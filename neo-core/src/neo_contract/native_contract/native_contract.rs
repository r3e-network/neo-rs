use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use NeoRust::builder::ScriptBuilder;
use crate::hardfork::Hardfork;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::contract_state::ContractState;
use crate::neo_contract::execution_context_state::ExecutionContextState;
use crate::neo_contract::key_builder::KeyBuilder;
use crate::neo_contract::manifest::contract_abi::ContractAbi;
use crate::neo_contract::manifest::contract_manifest::ContractManifest;
use crate::neo_contract::manifest::contract_permission::ContractPermission;
use crate::neo_contract::manifest::wild_card_container::WildcardContainer;
use crate::neo_contract::native_contract::contract_event_attribute::ContractEventAttribute;
use crate::neo_contract::native_contract::contract_method_metadata::ContractMethodMetadata;
use crate::neo_contract::nef_file::NefFile;
use crate::protocol_settings::ProtocolSettings;
use neo_type::H160;
use std::sync::Mutex;
use lazy_static::lazy_static;
use std::sync::atomic::{AtomicI32, Ordering};
use crate::cryptography::Helper;

lazy_static! {
    static ref CONTRACTS_LIST: Mutex<Vec<Arc<dyn NativeContract>>> = Mutex::new(Vec::new());
    static ref CONTRACTS_DICTIONARY: Mutex<HashMap<H160, Arc<dyn NativeContract>>> = Mutex::new(HashMap::new());
}

pub trait NativeContract: Send + Sync {
    fn name(&self) -> &str;
    fn active_in(&self) -> Option<Hardfork>;
    fn hash(&self) -> &H160;
    fn id(&self) -> i32;
    fn event_descriptors(&self) -> &[ContractEventAttribute];

    fn initialize(&self, engine: &mut ApplicationEngine, hard_fork: Option<Hardfork>) -> Result<(), Box<dyn std::error::Error>>;
    fn on_persist(&self, engine: &mut ApplicationEngine) -> Result<(), Box<dyn std::error::Error>>;
    fn post_persist(&self, engine: &mut ApplicationEngine) -> Result<(), Box<dyn std::error::Error>>;

    fn get_contract_state(&self, settings: &ProtocolSettings, block_height: u32) -> ContractState {
        self.get_contract_state_with_checker(
            |hf, height| settings.is_hardfork_enabled(hf, height),
            block_height,
        )
    }

    fn get_contract_state_with_checker(
        &self,
        hf_checker: impl Fn(Hardfork, u32) -> bool,
        block_height: u32,
    ) -> ContractState {
        let allowed_methods = self.get_allowed_methods(&hf_checker, block_height);

        // Compose NEF file
        let nef = NefFile {
            compiler: "neo-core-v3.0".to_string(),
            source: String::new(),
            tokens: Vec::new(),
            script: allowed_methods.script.clone(),
            checksum: 0,
        };

        // Compose manifest
        let mut manifest = ContractManifest {
            name: self.name().to_string(),
            groups: Vec::new(),
            supported_standards: Vec::new(),
            abi: ContractAbi {
                events: self
                    .events_descriptors()
                    .iter()
                    .filter(|&e| Self::is_active(e, &hf_checker, block_height))
                    .map(|e| e.descriptor.clone())
                    .collect(),
                methods: allowed_methods
                    .methods
                    .values()
                    .map(|m| m.descriptor.clone())
                    .collect(),
            },
            permissions: vec![ContractPermission::default()],
            trusts: WildcardContainer::create_wildcard(),
            extra: None,
        };

        self.on_manifest_compose(&mut manifest);

        ContractState {
            id: self.id(),
            nef,
            hash: self.hash(),
            manifest,
            update_counter: 0,
        }
    }

    fn is_initialize_block(
        &self,
        settings: &ProtocolSettings,
        index: u32,
    ) -> (bool, Vec<Hardfork>) {
        let mut hfs = Vec::new();

        for &hf in self.used_hardforks() {
            let active_in = settings.hardforks.get(&hf).copied().unwrap_or(0);
            if active_in == index {
                hfs.push(hf);
            }
        }

        if !hfs.is_empty() {
            return (true, hfs);
        }

        if index == 0 && self.active_in().is_none() {
            return (true, hfs);
        }

        (false, hfs)
    }

    fn is_active(&self, settings: &ProtocolSettings, block_height: u32) -> bool {
        match self.active_in() {
            None => true,
            Some(active_in) => {
                let active_height = settings.hardforks.get(&active_in).copied().unwrap_or(0);
                active_height <= block_height
            }
        }
    }

    fn check_committee(engine: &ApplicationEngine) -> bool {
        let committee_multi_sig_addr = NEO::get_committee_address(engine.snapshot_cache());
        engine.check_witness_internal(&committee_multi_sig_addr)
    }

    fn create_storage_key(&self, prefix: u8) -> KeyBuilder {
        KeyBuilder::new(self.id(), prefix)
    }

    fn invoke(&self, engine: &mut ApplicationEngine, version: u8) {
        if version != 0 {
            engine.throw_exception("InvalidOperationException", &format!("The native contract of version {} is not active.", version));
            return;
        }

        let native_contracts = engine.get_state(|| NativeContractsCache::new());
        let current_allowed_methods = native_contracts.get_allowed_methods(self, engine);
        let context = engine.current_context();
        let method = match current_allowed_methods.methods.get(&context.instruction_pointer()) {
            Some(m) => m,
            None => {
                engine.throw_exception("InvalidOperationException", "Method not found");
                return;
            }
        };

        if let Some(active_in) = method.active_in {
            if !engine.is_hardfork_enabled(active_in) {
                engine.throw_exception("InvalidOperationException", &format!("Cannot call this method before hardfork {:?}", active_in));
                return;
            }
        }

        if let Some(deprecated_in) = method.deprecated_in {
            if engine.is_hardfork_enabled(deprecated_in) {
                engine.throw_exception("InvalidOperationException", &format!("Cannot call this method after hardfork {:?}", deprecated_in));
                return;
            }
        }

        let state = context.get_state::<ExecutionContextState>();
        if !state.call_flags.contains(method.required_call_flags) {
            engine.throw_exception("InvalidOperationException", &format!("Cannot call this method with the flag {:?}", state.call_flags));
            return;
        }

        engine.add_fee(method.cpu_fee * engine.exec_fee_factor + method.storage_fee * engine.storage_price);

        let mut parameters = Vec::new();
        if method.need_application_engine {
            parameters.push(engine as *mut ApplicationEngine as *mut std::ffi::c_void);
        }
        if method.need_snapshot {
            parameters.push(engine.snapshot_cache() as *const _ as *mut std::ffi::c_void);
        }
        for i in 0..method.parameters.len() {
            let value = engine.convert(context.evaluation_stack().peek(i), method.parameters[i]);
            parameters.push(value as *mut std::ffi::c_void);
        }

        let result = unsafe { (method.handler)(parameters.as_ptr()) };

        for _ in 0..method.parameters.len() {
            context.evaluation_stack_mut().pop();
        }

        if !method.handler_return_type.is_void() {
            context.evaluation_stack_mut().push(engine.convert(result));
        }
    }


    fn initialize_async(&self, engine: &mut ApplicationEngine, hard_fork: Option<Hardfork>) {
        // Default implementation
    }

    fn on_persist_async(&self, engine: &mut ApplicationEngine) {
        // Default implementation
    }

    fn post_persist_async(&self, engine: &mut ApplicationEngine) {
        // Default implementation
    }

    // Helper methods
    fn method_descriptors(&self) -> &[ContractMethodMetadata];
    fn events_descriptors(&self) -> &[ContractEventAttribute];
    fn used_hardforks(&self) -> &HashSet<Hardfork>;

    fn get_allowed_methods(
        &self,
        hf_checker: &impl Fn(Hardfork, u32) -> bool,
        block_height: u32,
    ) -> CacheEntry {
        let mut methods = HashMap::new();
        let mut script = Vec::new();

        for method in self.method_descriptors() {
            if Self::is_active(method, hf_checker, block_height) {
                let offset = script.len();
                script.extend_from_slice(&[0]); // version
                methods.insert(offset, method.clone());
                script.extend_from_slice(&[0xE0, 0x01]); // SYSCALL
                script.extend_from_slice(&[0xC3]); // RET
            }
        }

        CacheEntry { methods, script }
    }

    fn on_manifest_compose(&self, manifest: &mut ContractManifest) {
        // Default implementation does nothing
    }

    fn is_active_method(
        method: &impl HardforkActivable,
        hf_checker: &impl Fn(Hardfork, u32) -> bool,
        block_height: u32,
    ) -> bool {
        match (method.active_in(), method.deprecated_in()) {
            (None, None) => true,
            (None, Some(deprecated)) => !hf_checker(deprecated, block_height),
            (Some(active), None) => hf_checker(active, block_height),
            (Some(active), Some(deprecated)) => {
                hf_checker(active, block_height) && !hf_checker(deprecated, block_height)
            }
        }
    }

}

pub struct BaseNativeContract {
    name: String,
    active_in: Option<Hardfork>,
    hash: H160,
    id: i32,
    method_descriptors: Vec<ContractMethodMetadata>,
    event_descriptors: Vec<ContractEventAttribute>,
    used_hardforks: HashSet<Hardfork>,
}

impl BaseNativeContract {
    pub fn new(name: String, active_in: Option<Hardfork>) -> Arc<Self> {
        let id = Self::generate_id();
        let hash = Helper::get_contract_hash(&H160::zero(), 0, &name);
        
        let method_descriptors = Self::get_method_descriptors();
        let event_descriptors = Self::get_event_descriptors();
        
        let used_hardforks = Self::calculate_used_hardforks(&method_descriptors, &event_descriptors, active_in);
        
        let contract = Arc::new(BaseNativeContract {
            name,
            active_in,
            hash,
            id,
            method_descriptors,
            event_descriptors,
            used_hardforks,
        });

        CONTRACTS_LIST.lock().unwrap().push(contract.clone());
        CONTRACTS_DICTIONARY.lock().unwrap().insert(hash, contract.clone());

        contract
    }

    fn generate_id() -> i32 {
        static ID_COUNTER: AtomicI32 = AtomicI32::new(0);
        ID_COUNTER.fetch_sub(1, Ordering::SeqCst)
    }

    fn get_method_descriptors() -> Vec<ContractMethodMetadata> {
        // Implementation to get method descriptors using Rust reflection or manual definition
        // This would replace the C# reflection logic
        unimplemented!()
    }

    fn get_event_descriptors() -> Vec<ContractEventAttribute> {
        // Implementation to get event descriptors
        // This would replace the C# reflection logic
        unimplemented!()
    }

    fn calculate_used_hardforks(
        method_descriptors: &[ContractMethodMetadata],
        event_descriptors: &[ContractEventAttribute],
        active_in: Option<Hardfork>,
    ) -> HashSet<Hardfork> {
        let mut hardforks = HashSet::new();
        
        for method in method_descriptors {
            if let Some(hf) = method.active_in {
                hardforks.insert(hf);
            }
            if let Some(hf) = method.deprecated_in {
                hardforks.insert(hf);
            }
        }

        for event in event_descriptors {
            if let Some(hf) = event.deprecated_in {
                hardforks.insert(hf);
            }
            if let Some(hf) = event.active_in {
                hardforks.insert(hf);
            }
        }

        if let Some(hf) = active_in {
            hardforks.insert(hf);
        }

        hardforks
    }
}

impl NativeContract for BaseNativeContract {
    fn name(&self) -> &str {
        &self.name
    }

    fn active_in(&self) -> Option<Hardfork> {
        self.active_in
    }

    fn hash(&self) -> &H160 {
        &self.hash
    }

    fn id(&self) -> i32 {
        self.id
    }

    fn method_descriptors(&self) -> &[ContractMethodMetadata] {
        &self.method_descriptors
    }

    fn event_descriptors(&self) -> &[ContractEventAttribute] {
        &self.event_descriptors
    }

    fn used_hardforks(&self) -> &HashSet<Hardfork> {
        &self.used_hardforks
    }

    fn get_allowed_methods(&self, hf_checker: &dyn Fn(Hardfork, u32) -> bool, block_height: u32) -> CacheEntry {
        let mut methods = HashMap::new();
        let mut script_builder = ScriptBuilder::new();

        for method in &self.method_descriptors {
            if Self::is_active(method, hf_checker, block_height) {
                let offset = script_builder.len();
                script_builder.push_integer(BigInt::from(0)); // version
                methods.insert(script_builder.len(), method.clone());
                script_builder.sys_call(ApplicationEngine::SYSTEM_CONTRACT_CALL_NATIVE);
                script_builder.op_code(&[OpCode::Ret]);
            }
        }

        CacheEntry {
            methods,
            script: script_builder.to_bytes(),
        }
    }

    fn get_contract_state(&self, hf_checker: &dyn Fn(Hardfork, u32) -> bool, block_height: u32) -> ContractState {
        let allowed_methods = self.get_allowed_methods(hf_checker, block_height);

        let nef = NefFile {
            compiler: "neo-core-v3.0".to_string(),
            source: String::new(),
            tokens: Vec::new(),
            script: allowed_methods.script.clone(),
            checksum: 0,
        };

        let manifest = ContractManifest {
            name: self.name.clone(),
            groups: Vec::new(),
            supported_standards: Vec::new(),
            abi: ContractAbi {
                methods: allowed_methods.methods.values()
                    .map(|m| m.descriptor.clone())
                    .collect(),
                events: self.event_descriptors.iter()
                    .filter(|e| Self::is_active(e, hf_checker, block_height))
                    .map(|e| e.descriptor.clone())
                    .collect(),
            },
            permissions: vec![ContractPermission::default_permission()],
            trusts: WildcardContainer::create(),
            extra: None,
        };

        self.on_manifest_compose(&mut manifest);

        ContractState {
            id: self.id,
            nef,
            hash: self.hash,
            manifest,
            update_counter: 0,
        }
    }

    fn on_manifest_compose(&self, _manifest: &mut ContractManifest) {
        // Default implementation does nothing
    }

    fn is_initialize_block(&self, settings: &ProtocolSettings, index: u32) -> (bool, Option<Vec<Hardfork>>) {
        let mut hfs = Vec::new();

        for &hf in &self.used_hardforks {
            let active_in = settings.hardforks.get(&hf).cloned().unwrap_or(0);
            if active_in == index {
                hfs.push(hf);
            }
        }

        if !hfs.is_empty() {
            return (true, Some(hfs));
        }

        if index == 0 && self.active_in.is_none() {
            return (true, Some(Vec::new()));
        }

        (false, None)
    }

    fn is_active(&self, settings: &ProtocolSettings, block_height: u32) -> bool {
        match self.active_in {
            None => true,
            Some(hf) => {
                let active_in = settings.hardforks.get(&hf).cloned().unwrap_or(0);
                active_in <= block_height
            }
        }
    }

    fn check_committee(engine: &ApplicationEngine) -> bool {
        let committee_multi_sig_addr = NEO::get_committee_address(engine.snapshot_cache());
        engine.check_witness_internal(&committee_multi_sig_addr)
    }

    fn create_storage_key(&self, prefix: u8) -> KeyBuilder {
        KeyBuilder::new(self.id, prefix)
    }

    fn invoke(&self, engine: &mut ApplicationEngine, version: u8) -> Result<(), Box<dyn std::error::Error>> {
        if version != 0 {
            return Err(format!("The native contract of version {} is not active.", version).into());
        }

        let native_contracts = engine.get_state::<NativeContractsCache>();
        let current_allowed_methods = native_contracts.get_allowed_methods(self, engine);

        let context = engine.current_context()?;
        let method = current_allowed_methods.methods.get(&context.instruction_pointer)
            .ok_or("Method not found")?;

        if let Some(active_in) = method.active_in {
            if !engine.is_hardfork_enabled(active_in) {
                return Err(format!("Cannot call this method before hardfork {:?}.", active_in).into());
            }
        }

        if let Some(deprecated_in) = method.deprecated_in {
            if engine.is_hardfork_enabled(deprecated_in) {
                return Err(format!("Cannot call this method after hardfork {:?}.", deprecated_in).into());
            }
        }

        let state = context.get_state::<ExecutionContextState>();
        if !state.call_flags.contains(method.required_call_flags) {
            return Err(format!("Cannot call this method with the flag {:?}.", state.call_flags).into());
        }

        engine.add_fee(method.cpu_fee * engine.exec_fee_factor + method.storage_fee * engine.storage_price);

        let mut parameters = Vec::new();
        if method.need_application_engine {
            parameters.push(engine);
        }
        if method.need_snapshot {
            parameters.push(engine.snapshot_cache());
        }
        for i in 0..method.parameters.len() {
            let param = engine.convert(context.evaluation_stack.peek(i), method.parameters[i])?;
            parameters.push(param);
        }

        let return_value = method.handler.call(self, &parameters)?;

        for _ in 0..method.parameters.len() {
            context.evaluation_stack.pop();
        }

        if method.handler.return_type() != "void" && method.handler.return_type() != "ContractTask" {
            context.evaluation_stack.push(engine.convert(return_value)?);
        }

        Ok(())
    }

    fn initialize(&self, _engine: &mut ApplicationEngine, _hard_fork: Option<Hardfork>) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn on_persist(&self, _engine: &mut ApplicationEngine) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn post_persist(&self, _engine: &mut ApplicationEngine) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

pub struct CacheEntry {
    pub methods: HashMap<usize, ContractMethodMetadata>,
    pub script: Vec<u8>,
}

pub trait HardforkActivable {
    fn active_in(&self) -> Option<Hardfork>;
    fn deprecated_in(&self) -> Option<Hardfork>;
}

// Implement named native contracts
lazy_static! {
    pub static ref CONTRACT_MANAGEMENT: Arc<dyn NativeContract> = BaseNativeContract::new("ContractManagement".to_string(), None);
    pub static ref STD_LIB: Arc<dyn NativeContract> = BaseNativeContract::new("StdLib".to_string(), None);
    pub static ref CRYPTO_LIB: Arc<dyn NativeContract> = BaseNativeContract::new("CryptoLib".to_string(), None);
    pub static ref LEDGER: Arc<dyn NativeContract> = BaseNativeContract::new("Ledger".to_string(), None);
    pub static ref NEO: Arc<dyn NativeContract> = BaseNativeContract::new("NEO".to_string(), None);
    pub static ref GAS: Arc<dyn NativeContract> = BaseNativeContract::new("GAS".to_string(), None);
    pub static ref POLICY: Arc<dyn NativeContract> = BaseNativeContract::new("Policy".to_string(), None);
    pub static ref ROLE_MANAGEMENT: Arc<dyn NativeContract> = BaseNativeContract::new("RoleManagement".to_string(), None);
    pub static ref ORACLE: Arc<dyn NativeContract> = BaseNativeContract::new("Oracle".to_string(), None);
}