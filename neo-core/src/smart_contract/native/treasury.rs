use crate::error::{CoreError as Error, CoreResult as Result};
use crate::hardfork::Hardfork;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::native::method_macros::neo_native_methods;
use crate::smart_contract::native::{NativeContract, NativeMethod};
use crate::UInt160;

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
        let hash = crate::smart_contract::helper::Helper::get_contract_hash(
            &UInt160::zero(),
            0,
            Self::NAME,
        );

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

impl Default for TreasuryContract {
    fn default() -> Self {
        Self::new()
    }
}

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
        _settings: &crate::protocol_settings::ProtocolSettings,
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
                Ok(vec![if check { 1 } else { 0 }])
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
