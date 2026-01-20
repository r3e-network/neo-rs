use crate::error::{CoreError as Error, CoreResult as Result};
use crate::hardfork::Hardfork;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::native::{NativeContract, NativeMethod};
use crate::smart_contract::ContractParameterType;
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

        let methods = vec![
            NativeMethod::safe(
                "verify".to_string(),
                1 << 5,
                Vec::new(),
                ContractParameterType::Boolean,
            )
            .with_required_call_flags(CallFlags::READ_STATES),
            NativeMethod::safe(
                "onNEP17Payment".to_string(),
                1 << 5,
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                    ContractParameterType::Any,
                ],
                ContractParameterType::Void,
            )
            .with_parameter_names(vec![
                "from".to_string(),
                "amount".to_string(),
                "data".to_string(),
            ]),
            NativeMethod::safe(
                "onNEP11Payment".to_string(),
                1 << 5,
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                    ContractParameterType::ByteArray,
                    ContractParameterType::Any,
                ],
                ContractParameterType::Void,
            )
            .with_parameter_names(vec![
                "from".to_string(),
                "amount".to_string(),
                "tokenId".to_string(),
                "data".to_string(),
            ]),
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
