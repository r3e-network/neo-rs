//! Oracle `NativeContract` implementation.
//!
//! Migrated from `neo-core/src/smart_contract/native/oracle_contract/native_impl.rs`.
//! Import adaptation: `crate::error::*` → `neo_smart_contract::error::*`;
//! `crate::hardfork::Hardfork` → `neo_protocol::Hardfork`;
//! `crate::impl_native_contract` → `neo_smart_contract::impl_native_contract`;
//! `crate::persistence::read_only_store::*` →
//! `neo_storage::persistence::read_only_store::*`;
//! `crate::protocol_settings::ProtocolSettings` → `neo_protocol::ProtocolSettings`;
//! `crate::smart_contract::StorageItem` → `neo_storage::StorageItem`;
//! `crate::smart_contract::application_engine::*` →
//! `neo_smart_contract::application_engine::*`; `crate::smart_contract::manifest::*`
//! → `neo_smart_contract::manifest::*`; `crate::smart_contract::native::*` →
//! `crate::native::*`; `crate::impl_default_via_new!` →
//! `neo_storage::impl_default_via_new!`.

use super::{DEFAULT_PRICE, OracleContract};
use crate::native::NativeContract;
use neo_protocol::Hardfork;
use neo_protocol::ProtocolSettings;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::error::CoreResult as Result;
use neo_smart_contract::impl_native_contract;
use neo_smart_contract::manifest::ContractEventDescriptor;
use neo_storage::StorageItem;
use neo_storage::persistence::read_only_store::ReadOnlyStoreGeneric;
use num_bigint::BigInt;

impl NativeContract for OracleContract {
    impl_native_contract!(hash, "OracleContract", methods);

    fn id(&self) -> i32 {
        self.id
    }

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

    fn events(
        &self,
        _settings: &ProtocolSettings,
        _block_height: u32,
    ) -> Vec<ContractEventDescriptor> {
        Self::event_descriptors()
    }

    fn post_persist(&self, engine: &mut ApplicationEngine) -> Result<()> {
        let completed_response_ids = self.cleanup_persisted_responses(engine)?;
        self.reward_oracle_nodes(engine, &completed_response_ids)
    }
}

neo_storage::impl_default_via_new!(OracleContract);
