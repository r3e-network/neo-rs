// Copyright (C) 2015-2024 The Neo Project.
//
// native_contract.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo::io::*;
use neo::smart_contract::manifest::*;
use neo::vm::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// The base struct of all native contracts.
pub struct NativeContract {
    name: String,
    active_in: Option<Hardfork>,
    hash: UInt160,
    id: i32,
    method_descriptors: Vec<ContractMethodMetadata>,
    event_descriptors: Vec<ContractEventAttribute>,
    used_hardforks: HashSet<Hardfork>,
}

impl NativeContract {
    pub fn new(name: String, active_in: Option<Hardfork>, id: i32) -> Self {
        let hash = Helper::get_contract_hash(&UInt160::zero(), 0, &name);
        
        let method_descriptors = Self::get_method_descriptors();
        let event_descriptors = Self::get_event_descriptors();
        
        let used_hardforks = Self::calculate_used_hardforks(&method_descriptors, &event_descriptors, active_in);
        
        NativeContract {
            name,
            active_in,
            hash,
            id,
            method_descriptors,
            event_descriptors,
            used_hardforks,
        }
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

    pub fn get_allowed_methods(&self, hf_checker: &dyn Fn(Hardfork, u32) -> bool, block_height: u32) -> CacheEntry {
        let mut methods = HashMap::new();
        let mut script_builder = ScriptBuilder::new();

        for method in &self.method_descriptors {
            if Self::is_active(method, hf_checker, block_height) {
                let offset = script_builder.len();
                script_builder.emit_push(0); // version
                methods.insert(script_builder.len(), method.clone());
                script_builder.emit_syscall(ApplicationEngine::SYSTEM_CONTRACT_CALL_NATIVE);
                script_builder.emit(OpCode::RET);
            }
        }

        CacheEntry {
            methods,
            script: script_builder.to_vec(),
        }
    }

    pub fn get_contract_state(&self, hf_checker: &dyn Fn(Hardfork, u32) -> bool, block_height: u32) -> ContractState {
        let allowed_methods = self.get_allowed_methods(hf_checker, block_height);

        let nef = NefFile {
            compiler: "neo-core-v3.0".to_string(),
            source: String::new(),
            tokens: Vec::new(),
            script: allowed_methods.script.clone(),
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
        }
    }

    fn on_manifest_compose(&self, _manifest: &mut ContractManifest) {
        // Default implementation does nothing
    }

    pub fn is_initialize_block(&self, settings: &ProtocolSettings, index: u32) -> (bool, Option<Vec<Hardfork>>) {
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

    pub fn is_active(&self, settings: &ProtocolSettings, block_height: u32) -> bool {
        match self.active_in {
            None => true,
            Some(hf) => {
                let active_in = settings.hardforks.get(&hf).cloned().unwrap_or(0);
                active_in <= block_height
            }
        }
    }

    fn is_active<T: HardforkActivable>(item: &T, hf_checker: &dyn Fn(Hardfork, u32) -> bool, block_height: u32) -> bool {
        match (item.active_in(), item.deprecated_in()) {
            (None, None) => true,
            (None, Some(deprecated)) => !hf_checker(deprecated, block_height),
            (Some(active), None) => hf_checker(active, block_height),
            (Some(active), Some(deprecated)) => hf_checker(active, block_height) && !hf_checker(deprecated, block_height),
        }
    }

    pub fn check_committee(engine: &ApplicationEngine) -> bool {
        let committee_multi_sig_addr = NEO::get_committee_address(engine.snapshot_cache());
        engine.check_witness_internal(&committee_multi_sig_addr)
    }

    pub fn create_storage_key(&self, prefix: u8) -> KeyBuilder {
        KeyBuilder::new(self.id, prefix)
    }

    pub async fn invoke(&self, engine: &mut ApplicationEngine, version: u8) -> Result<(), Box<dyn std::error::Error>> {
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

    pub async fn initialize(&self, _engine: &mut ApplicationEngine, _hard_fork: Option<Hardfork>) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    pub async fn on_persist(&self, _engine: &mut ApplicationEngine) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    pub async fn post_persist(&self, _engine: &mut ApplicationEngine) -> Result<(), Box<dyn std::error::Error>> {
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

#[derive(Clone, Debug)]
pub struct ContractMethodMetadata {
    pub name: String,
    pub parameters: Vec<ContractParameterType>,
    pub return_type: ContractParameterType,
    pub cpu_fee: i64,
    pub storage_fee: i64,
    pub need_application_engine: bool,
    pub need_snapshot: bool,
    pub active_in: Option<Hardfork>,
    pub deprecated_in: Option<Hardfork>,
    pub handler: Arc<dyn Fn(&NativeContract, &[StackItem]) -> Result<StackItem, String> + Send + Sync>,
}

#[derive(Clone, Debug)]
pub struct ContractEventAttribute {
    pub name: String,
    pub parameters: Vec<ContractParameterType>,
}

pub struct ApplicationEngine {
    pub snapshot_cache: StorageContext,
    pub exec_fee_factor: i64,
    pub storage_price: i64,
    // Add other necessary fields
}

impl ApplicationEngine {
    pub fn add_fee(&mut self, fee: i64) {
        // Implementation for adding fee
    }

    pub fn convert(&self, item: StackItem, param_type: ContractParameterType) -> Result<StackItem, String> {
        // Implementation for converting stack items
        unimplemented!()
    }

    // Add other necessary methods
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hardfork {
    // Define hardfork variants
}

#[derive(Clone, Copy)]
pub enum ContractParameterType {
    // Define parameter types
}

pub struct StorageContext {
    // Define storage context
}

// Additional implementations as needed
