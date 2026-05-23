use super::{OracleContract, DEFAULT_PRICE};
use crate::error::CoreResult as Result;
use crate::hardfork::Hardfork;
use crate::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::manifest::ContractEventDescriptor;
use crate::smart_contract::native::{NativeContract, NativeMethod};
use crate::smart_contract::StorageItem;
use crate::UInt160;
use num_bigint::BigInt;

impl NativeContract for OracleContract {
    fn supported_standards(&self, settings: &ProtocolSettings, block_height: u32) -> Vec<String> {
        if settings.is_hardfork_enabled(Hardfork::HfFaun, block_height) {
            vec!["NEP-30".to_string()]
        } else {
            Vec::new()
        }
    }

    fn activations(&self) -> Vec<Hardfork> {
        vec![Hardfork::HfFaun]
    }

    fn initialize(&self, engine: &mut ApplicationEngine) -> Result<()> {
        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();

        if snapshot_ref.try_get(&self.request_id_key()).is_none() {
            snapshot_ref.add(self.request_id_key(), StorageItem::from_bytes(Vec::new()));
        }

        if snapshot_ref.try_get(&self.price_key()).is_none() {
            let price_bytes = BigInt::from(DEFAULT_PRICE).to_signed_bytes_le();
            snapshot_ref.add(self.price_key(), StorageItem::from_bytes(price_bytes));
        }

        Ok(())
    }

    fn id(&self) -> i32 {
        self.id
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn name(&self) -> &str {
        "OracleContract"
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn events(
        &self,
        _settings: &ProtocolSettings,
        _block_height: u32,
    ) -> Vec<ContractEventDescriptor> {
        Self::event_descriptors()
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        self.invoke_method(engine, method, args)
    }

    fn post_persist(&self, engine: &mut ApplicationEngine) -> Result<()> {
        let completed_response_ids = self.cleanup_persisted_responses(engine)?;
        self.reward_oracle_nodes(engine, &completed_response_ids)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Default for OracleContract {
    fn default() -> Self {
        Self::new()
    }
}
