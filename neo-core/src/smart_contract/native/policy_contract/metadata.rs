use super::PolicyContract;
use crate::hardfork::Hardfork;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::manifest::ContractEventDescriptor;
use crate::smart_contract::native::metadata_macros::event_descriptor;
use crate::smart_contract::native::method_macros::{
    neo_native_method_dispatch, neo_native_method_metadata,
};
use crate::smart_contract::native::NativeMethod;

macro_rules! policy_method_table {
    ($callback:ident; $($args:tt)*) => {
        $callback! {
            $($args)*
            ;
            {
                safe "getFeePerByte", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = Integer => engine_only get_fee_per_byte;
                safe "getExecFeeFactor", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = Integer => engine_only get_exec_fee_factor;
                safe "getExecPicoFeeFactor", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = Integer, active = HfFaun => engine_only get_exec_pico_fee_factor;
                safe "getStoragePrice", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = Integer => engine_only get_storage_price;
                safe "getMillisecondsPerBlock", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = Integer, active = HfEchidna => engine_only get_milliseconds_per_block;
                safe "getMaxValidUntilBlockIncrement", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = Integer, active = HfEchidna => engine_only get_max_valid_until_block_increment;
                safe "getMaxTraceableBlocks", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = Integer, active = HfEchidna => engine_only get_max_traceable_blocks;
                safe "getAttributeFee", fee = Self::CPU_FEE, flags = [READ_STATES], params = [Integer], returns = Integer, deprecated = HfEchidna, names = ["attributeType"] => engine get_attribute_fee;
                safe "getAttributeFee", fee = Self::CPU_FEE, flags = [READ_STATES], params = [Integer], returns = Integer, active = HfEchidna, names = ["attributeType"] => engine get_attribute_fee;
                unsafe "setFeePerByte", fee = Self::CPU_FEE, flags = [STATES], params = [Integer], returns = Void, names = ["value"] => engine set_fee_per_byte;
                unsafe "setExecFeeFactor", fee = Self::CPU_FEE, flags = [STATES], params = [Integer], returns = Void, names = ["value"] => engine set_exec_fee_factor;
                unsafe "setStoragePrice", fee = Self::CPU_FEE, flags = [STATES], params = [Integer], returns = Void, names = ["value"] => engine set_storage_price;
                unsafe "setMillisecondsPerBlock", fee = Self::CPU_FEE, flags = [STATES, ALLOW_NOTIFY], params = [Integer], returns = Void, active = HfEchidna, names = ["value"] => engine set_milliseconds_per_block;
                unsafe "setMaxValidUntilBlockIncrement", fee = Self::CPU_FEE, flags = [STATES], params = [Integer], returns = Void, active = HfEchidna, names = ["value"] => engine set_max_valid_until_block_increment;
                unsafe "setMaxTraceableBlocks", fee = Self::CPU_FEE, flags = [STATES], params = [Integer], returns = Void, active = HfEchidna, names = ["value"] => engine set_max_traceable_blocks;
                unsafe "setAttributeFee", fee = Self::CPU_FEE, flags = [STATES], params = [Integer, Integer], returns = Void, deprecated = HfEchidna, names = ["attributeType", "value"] => engine set_attribute_fee;
                unsafe "setAttributeFee", fee = Self::CPU_FEE, flags = [STATES], params = [Integer, Integer], returns = Void, active = HfEchidna, names = ["attributeType", "value"] => engine set_attribute_fee;
                safe "isBlocked", fee = Self::CPU_FEE, flags = [READ_STATES], params = [Hash160], returns = Boolean, names = ["account"] => engine is_blocked;
                unsafe "blockAccount", fee = Self::CPU_FEE, flags = [STATES], params = [Hash160], returns = Boolean, deprecated = HfFaun, names = ["account"] => engine block_account;
                unsafe "blockAccount", fee = Self::CPU_FEE, flags = [STATES, ALLOW_NOTIFY], params = [Hash160], returns = Boolean, active = HfFaun, names = ["account"] => engine block_account;
                unsafe "unblockAccount", fee = Self::CPU_FEE, flags = [STATES], params = [Hash160], returns = Boolean, names = ["account"] => engine unblock_account;
                safe "getBlockedAccounts", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = InteropInterface, active = HfFaun => engine_only get_blocked_accounts;
                unsafe "setWhitelistFeeContract", fee = Self::CPU_FEE, flags = [STATES, ALLOW_NOTIFY], params = [Hash160, String, Integer, Integer], returns = Void, active = HfFaun, names = ["contractHash", "method", "argCount", "fixedFee"] => engine set_whitelist_fee_contract;
                unsafe "removeWhitelistFeeContract", fee = Self::CPU_FEE, flags = [STATES, ALLOW_NOTIFY], params = [Hash160, String, Integer], returns = Void, active = HfFaun, names = ["contractHash", "method", "argCount"] => engine remove_whitelist_fee_contract;
                safe "getWhitelistFeeContracts", fee = Self::CPU_FEE, flags = [READ_STATES], params = [], returns = InteropInterface, active = HfFaun => engine_only get_whitelist_fee_contracts;
                unsafe "recoverFund", fee = Self::CPU_FEE, flags = [ALL], params = [Hash160, Hash160], returns = Boolean, active = HfFaun, names = ["account", "token"] => engine recover_fund;
            }
        }
    };
}

impl PolicyContract {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        policy_method_table!(neo_native_method_metadata;)
    }

    pub(super) fn dispatch_method(
        &self,
        engine: &mut crate::smart_contract::ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> crate::error::CoreResult<Vec<u8>> {
        policy_method_table!(
            neo_native_method_dispatch;
            self,
            engine,
            method,
            args,
            aliases = [],
            unknown = |method| crate::error::CoreError::native_contract(format!("Unknown method: {method}"))
        )
    }

    pub(super) fn event_descriptors(
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Vec<ContractEventDescriptor> {
        if !settings.is_hardfork_enabled(Hardfork::HfEchidna, block_height) {
            return Vec::new();
        }

        let mut events = vec![event_descriptor!(
            Self::MILLISECONDS_PER_BLOCK_CHANGED_EVENT_NAME,
            expect = "MillisecondsPerBlockChanged",
            ["old" => Integer, "new" => Integer]
        )];

        if settings.is_hardfork_enabled(Hardfork::HfFaun, block_height) {
            events.push(event_descriptor!(
                "WhitelistFeeChanged",
                [
                    "contract" => Hash160,
                    "method" => String,
                    "argCount" => Integer,
                    "fee" => Any,
                ]
            ));

            events.push(event_descriptor!(
                "RecoveredFund",
                ["account" => Hash160]
            ));
        }

        events
    }
}
