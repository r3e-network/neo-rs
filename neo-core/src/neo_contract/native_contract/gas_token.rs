use std::collections::HashSet;
use std::error::Error;
use async_trait::async_trait;
use neo_proc_macros::contract_method;
use crate::contract::Contract;
use crate::hardfork::Hardfork;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::contract_state::ContractState;
use crate::neo_contract::contract_task::ContractTask;
use crate::neo_contract::interop_parameter_descriptor::InteropParameterType::BigInt;
use crate::neo_contract::key_builder::KeyBuilder;
use crate::neo_contract::manifest::contract_manifest::ContractManifest;
use crate::neo_contract::native_contract::{AccountState, FungibleToken, NativeContract};
use crate::neo_contract::native_contract::contract_event_attribute::ContractEventAttribute;
use crate::neo_contract::native_contract::contract_method_metadata::ContractMethodMetadata;
use crate::neo_contract::native_contract::native_contract::{CacheEntry, NEO};
use crate::protocol_settings::ProtocolSettings;
use neo_type::H160;

/// Represents the GAS token in the NEO system.
pub struct GasToken;


impl GasToken {
    pub fn new() -> Self {
        Self
    }

    #[contract_method(cpu_fee = 1 << 15)]
    pub fn symbol(&self) -> String {
        self.token.symbol()
    }

    #[contract_method(cpu_fee = 1 << 15)]
    pub fn decimals(&self) -> u8 {
        self.token.decimals()
    }

    pub(crate) fn initialize(&self, engine: &mut ApplicationEngine, hardfork: Option<Hardfork>) -> ContractTask {
        if hardfork == Some(self.active_in()) {
            let account = Contract::get_bft_address(&engine.protocol_settings().standby_validators);
            self.token.mint(engine, &account, engine.protocol_settings().initial_gas_distribution, false)
        } else {
            ContractTask::completed()
        }
    }

    async fn on_persist_async(&self, engine: &mut ApplicationEngine) -> Result<(), String> {
        let mut total_network_fee = 0;
        for tx in engine.persisting_block().transactions() {
            self.token.burn(engine, &tx.sender(), tx.system_fee() + tx.network_fee()).await?;
            total_network_fee += tx.network_fee();
        }
        let validators = NEO::get_next_block_validators(&engine.snapshot_cache, engine.protocol_settings().validators_count);
        let primary = Contract::create_signature_redeem_script(&validators[engine.persisting_block().primary_index]).to_script_hash();
        self.token.mint(engine, &primary, total_network_fee, false).await
    }

    fn active_in(&self) -> Hardfork {
        Hardfork::HF_Cockatrice
    }
}

#[async_trait]
impl NativeContract for GasToken {
    async fn initialize_async(&self, engine: &ApplicationEngine, hardfork: Option<Hardfork>) -> ContractTask {
        if let Some(hardfork) = hardfork {
            if hardfork == self.active_in() {
                let account = "some_account"; // Replace with actual logic
                return self.mint(engine, account, BigInt::from(100000000), false).await;
            }
        }
        ContractTask::completed_task()
    }

    async fn on_persist_async(&self, engine: &ApplicationEngine) -> ContractTask {
        let total_network_fee = BigInt::from(0);
        // Logic for on_persist_async
        ContractTask::completed_task()
    }

    async fn post_persist_async(&self, engine: &ApplicationEngine) -> ContractTask {
        ContractTask::completed_task()
    }

    fn name(&self) -> &str {
        todo!()
    }

    fn active_in(&self) -> Option<Hardfork> {
        todo!()
    }

    fn hash(&self) -> &H160 {
        todo!()
    }

    fn id(&self) -> i32 {
        todo!()
    }

    fn method_descriptors(&self) -> &[ContractMethodMetadata] {
        todo!()
    }

    fn event_descriptors(&self) -> &[ContractEventAttribute] {
        todo!()
    }

    fn used_hardforks(&self) -> &HashSet<Hardfork> {
        todo!()
    }

    fn get_allowed_methods(&self, hf_checker: &dyn Fn(Hardfork, u32) -> bool, block_height: u32) -> CacheEntry {
        todo!()
    }

    fn get_contract_state(&self, hf_checker: &dyn Fn(Hardfork, u32) -> bool, block_height: u32) -> ContractState {
        todo!()
    }

    fn on_manifest_compose(&self, manifest: &mut ContractManifest) {
        todo!()
    }

    fn is_initialize_block(&self, settings: &ProtocolSettings, index: u32) -> (bool, Option<Vec<Hardfork>>) {
        todo!()
    }

    fn is_active(&self, settings: &ProtocolSettings, block_height: u32) -> bool {
        todo!()
    }

    fn check_committee(engine: &ApplicationEngine) -> bool {
        todo!()
    }

    fn create_storage_key(&self, prefix: u8) -> KeyBuilder {
        todo!()
    }

    fn invoke(&self, engine: &mut ApplicationEngine, version: u8) -> Result<(), Box<dyn Error>> {
        todo!()
    }

    fn initialize(&self, engine: &mut ApplicationEngine, hard_fork: Option<Hardfork>) -> Result<(), Box<dyn Error>> {
        todo!()
    }

    fn on_persist(&self, engine: &mut ApplicationEngine) -> Result<(), Box<dyn Error>> {
        todo!()
    }

    fn post_persist(&self, engine: &mut ApplicationEngine) -> Result<(), Box<dyn Error>> {
        todo!()
    }

    fn events_descriptors(&self) -> &[ContractEventAttribute] {
        todo!()
    }
}

#[async_trait]
impl FungibleToken for GasToken {
    type State = ();

    fn factor(&self) -> &BigInt {
        todo!()
    }

    fn symbol(&self) -> &str {
        "GAS"
    }

    fn decimals(&self) -> u8 {
        8
    }

    async fn mint(&self, engine: &ApplicationEngine, account: &str, amount: BigInteger, call_on_payment: bool) -> ContractTask {
        // Mint logic here
        ContractTask::completed_task()
    }

    async fn burn(&self, engine: &ApplicationEngine, account: &str, amount: BigInteger) -> ContractTask {
        // Burn logic here
        ContractTask::completed_task()
    }
}