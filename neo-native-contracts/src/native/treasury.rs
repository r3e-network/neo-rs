//! The Treasury native contract.
//!
//! Migrated from `neo-core/src/smart_contract/native/treasury.rs`.
//!
//! Import adaptation:
//! - `crate::UInt160` → `neo_primitives::UInt160`
//! - `crate::error::{CoreError, CoreResult}` → `neo_smart_contract::error::{...}`
//! - `crate::hardfork::Hardfork` → `neo_protocol::Hardfork`
//! - `crate::smart_contract::application_engine::ApplicationEngine` → `neo_smart_contract::application_engine::ApplicationEngine`
//! - `crate::smart_contract::native::method_macros::neo_native_methods` → `super::method_macros::neo_native_methods`
//! - `crate::smart_contract::native::{NativeContract, NativeMethod}` → `super::{NativeContract, NativeMethod}` (re-exported by the native module root)
//! - `crate::smart_contract::helper::Helper` → `neo_smart_contract::helper::Helper`
//! - `crate::protocol_settings::ProtocolSettings` → `neo_protocol::ProtocolSettings`
//! - `crate::impl_default_via_new!` → `neo_storage::impl_default_via_new!` (the macro is `#[macro_export]`-ed from the `neo-storage` crate root)

use super::method_macros::neo_native_methods;
use super::{NativeContract, NativeMethod};
use neo_primitives::UInt160;
use neo_protocol::Hardfork;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::error::{CoreError as Error, CoreResult as Result};

/// The Treasury native contract.
pub struct TreasuryContract {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl TreasuryContract {
    const ID: i32 = -11;
    const NAME: &'static str = "Treasury";

    /// Creates a new Treasury contract.
    pub fn new() -> Self {
        let hash =
            neo_smart_contract::helper::Helper::get_contract_hash(&UInt160::zero(), 0, Self::NAME);

        let methods = neo_native_methods![
            safe "verify", fee = 1 << 5, flags = [READ_STATES], params = [], returns = Boolean;
            safe "onNEP17Payment", fee = 1 << 5, flags = [], params = [Hash160, Integer, Any], returns = Void, names = ["from", "amount", "data"];
            safe "onNEP11Payment", fee = 1 << 5, flags = [], params = [Hash160, Integer, ByteArray, Any], returns = Void, names = ["from", "amount", "tokenId", "data"];
        ];

        Self {
            id: Self::ID,
            hash,
            methods,
        }
    }
}

neo_storage::impl_default_via_new!(TreasuryContract);

impl NativeContract for TreasuryContract {
    fn id(&self) -> i32 {
        self.id
    }
    fn hash(&self) -> UInt160 {
        self.hash
    }
    fn name(&self) -> &str {
        Self::NAME
    }
    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }
    fn active_in(&self) -> Option<Hardfork> {
        Some(Hardfork::HfFaun)
    }

    fn activations(&self) -> Vec<Hardfork> {
        vec![Hardfork::HfFaun]
    }

    fn supported_standards(
        &self,
        _settings: &neo_protocol::ProtocolSettings,
        _block_height: u32,
    ) -> Vec<String> {
        vec![
            "NEP-26".to_string(),
            "NEP-27".to_string(),
            "NEP-30".to_string(),
        ]
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        _args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        match method {
            "verify" => {
                let check = engine.check_committee_witness()?;
                Ok(vec![u8::from(check)])
            }
            "onNEP17Payment" => Ok(Vec::new()),
            "onNEP11Payment" => Ok(Vec::new()),
            _ => Err(Error::invalid_operation(format!(
                "Method {method} not found in Treasury"
            ))),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
