use super::PolicyContract;
use crate::hardfork::Hardfork;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::manifest::{ContractEventDescriptor, ContractParameterDefinition};
use crate::smart_contract::native::method_macros::neo_native_methods;
use crate::smart_contract::native::NativeMethod;
use crate::smart_contract::ContractParameterType;

impl PolicyContract {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        neo_native_methods![
            safe "getFeePerByte", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = Integer;
            safe "getExecFeeFactor", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = Integer;
            safe "getExecPicoFeeFactor", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = Integer, active = HfFaun;
            safe "getStoragePrice", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = Integer;
            safe "getMillisecondsPerBlock", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = Integer, active = HfEchidna;
            safe "getMaxValidUntilBlockIncrement", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = Integer, active = HfEchidna;
            safe "getMaxTraceableBlocks", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = Integer, active = HfEchidna;
            safe "getAttributeFee", fee = Self::CPU_FEE, flags = [READ_STATES], params = [Integer], returns = Integer, deprecated = HfEchidna, names = ["attributeType"];
            safe "getAttributeFee", fee = Self::CPU_FEE, flags = [READ_STATES], params = [Integer], returns = Integer, active = HfEchidna, names = ["attributeType"];
            unsafe "setFeePerByte", fee = Self::CPU_FEE, flags = [STATES], params = [Integer], returns = Void, names = ["value"];
            unsafe "setExecFeeFactor", fee = Self::CPU_FEE, flags = [STATES], params = [Integer], returns = Void, names = ["value"];
            unsafe "setStoragePrice", fee = Self::CPU_FEE, flags = [STATES], params = [Integer], returns = Void, names = ["value"];
            unsafe "setMillisecondsPerBlock", fee = Self::CPU_FEE, flags = [STATES, ALLOW_NOTIFY], params = [Integer], returns = Void, active = HfEchidna, names = ["value"];
            unsafe "setMaxValidUntilBlockIncrement", fee = Self::CPU_FEE, flags = [STATES], params = [Integer], returns = Void, active = HfEchidna, names = ["value"];
            unsafe "setMaxTraceableBlocks", fee = Self::CPU_FEE, flags = [STATES], params = [Integer], returns = Void, active = HfEchidna, names = ["value"];
            unsafe "setAttributeFee", fee = Self::CPU_FEE, flags = [STATES], params = [Integer, Integer], returns = Void, deprecated = HfEchidna, names = ["attributeType", "value"];
            unsafe "setAttributeFee", fee = Self::CPU_FEE, flags = [STATES], params = [Integer, Integer], returns = Void, active = HfEchidna, names = ["attributeType", "value"];
            safe "isBlocked", fee = Self::CPU_FEE, flags = [READ_STATES], params = [Hash160], returns = Boolean, names = ["account"];
            unsafe "blockAccount", fee = Self::CPU_FEE, flags = [STATES], params = [Hash160], returns = Boolean, deprecated = HfFaun, names = ["account"];
            unsafe "blockAccount", fee = Self::CPU_FEE, flags = [STATES, ALLOW_NOTIFY], params = [Hash160], returns = Boolean, active = HfFaun, names = ["account"];
            unsafe "unblockAccount", fee = Self::CPU_FEE, flags = [STATES], params = [Hash160], returns = Boolean, names = ["account"];
            safe "getBlockedAccounts", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = InteropInterface, active = HfFaun;
            unsafe "setWhitelistFeeContract", fee = Self::CPU_FEE, flags = [STATES, ALLOW_NOTIFY], params = [Hash160, String, Integer, Integer], returns = Void, active = HfFaun, names = ["contractHash", "method", "argCount", "fixedFee"];
            unsafe "removeWhitelistFeeContract", fee = Self::CPU_FEE, flags = [STATES, ALLOW_NOTIFY], params = [Hash160, String, Integer], returns = Void, active = HfFaun, names = ["contractHash", "method", "argCount"];
            safe "getWhitelistFeeContracts", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = InteropInterface, active = HfFaun;
            unsafe "recoverFund", fee = Self::CPU_FEE, flags = [ALL], params = [Hash160, Hash160], returns = Boolean, active = HfFaun, names = ["account", "token"];
        ]
    }

    pub(super) fn event_descriptors(
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Vec<ContractEventDescriptor> {
        if !settings.is_hardfork_enabled(Hardfork::HfEchidna, block_height) {
            return Vec::new();
        }

        let mut events = vec![ContractEventDescriptor::new(
            Self::MILLISECONDS_PER_BLOCK_CHANGED_EVENT_NAME.to_string(),
            vec![
                ContractParameterDefinition::new("old".to_string(), ContractParameterType::Integer)
                    .expect("MillisecondsPerBlockChanged.old"),
                ContractParameterDefinition::new("new".to_string(), ContractParameterType::Integer)
                    .expect("MillisecondsPerBlockChanged.new"),
            ],
        )
        .expect("MillisecondsPerBlockChanged event descriptor")];

        if settings.is_hardfork_enabled(Hardfork::HfFaun, block_height) {
            events.push(
                ContractEventDescriptor::new(
                    "WhitelistFeeChanged".to_string(),
                    vec![
                        ContractParameterDefinition::new(
                            "contract".to_string(),
                            ContractParameterType::Hash160,
                        )
                        .expect("WhitelistFeeChanged.contract"),
                        ContractParameterDefinition::new(
                            "method".to_string(),
                            ContractParameterType::String,
                        )
                        .expect("WhitelistFeeChanged.method"),
                        ContractParameterDefinition::new(
                            "argCount".to_string(),
                            ContractParameterType::Integer,
                        )
                        .expect("WhitelistFeeChanged.argCount"),
                        ContractParameterDefinition::new(
                            "fee".to_string(),
                            ContractParameterType::Any,
                        )
                        .expect("WhitelistFeeChanged.fee"),
                    ],
                )
                .expect("WhitelistFeeChanged event descriptor"),
            );

            events.push(
                ContractEventDescriptor::new(
                    "RecoveredFund".to_string(),
                    vec![ContractParameterDefinition::new(
                        "account".to_string(),
                        ContractParameterType::Hash160,
                    )
                    .expect("RecoveredFund.account")],
                )
                .expect("RecoveredFund event descriptor"),
            );
        }

        events
    }
}
