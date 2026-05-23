//! Policy contract native implementation.
//!
//! Mirrors the behaviour of `Neo.SmartContract.Native.PolicyContract` from the
//! Neo N3 C# reference node.

use crate::error::{CoreError as Error, CoreResult as Result};
use crate::hardfork::Hardfork;
use crate::neo_config::ADDRESS_SIZE;
use crate::neo_vm::{ExecutionEngineLimits, StackItem};
use crate::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::find_options::FindOptions;
use crate::smart_contract::manifest::{ContractEventDescriptor, ContractParameterDefinition};
use crate::smart_contract::native::{NativeContract, NativeMethod};
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::{ContractParameterType, StorageItem};
use crate::UInt160;
use neo_primitives::TransactionAttributeType;
use neo_vm_rs::StackValue;
use num_bigint::{BigInt, Sign};
use num_traits::{ToPrimitive, Zero};
use std::any::Any;

/// Whitelisted fee contract info.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhitelistedContract {
    /// Contract hash.
    pub contract_hash: UInt160,
    /// Method name.
    pub method: String,
    /// Argument count.
    pub arg_count: u32,
    /// Fixed fee in datoshi.
    pub fixed_fee: i64,
}

impl Default for WhitelistedContract {
    fn default() -> Self {
        Self {
            contract_hash: UInt160::zero(),
            method: String::new(),
            arg_count: 0,
            fixed_fee: 0,
        }
    }
}

impl WhitelistedContract {
    fn stack_value_to_bigint(value: &StackValue) -> Option<BigInt> {
        match value {
            StackValue::Integer(value) => Some(BigInt::from(*value)),
            StackValue::Boolean(value) => Some(BigInt::from(i32::from(*value))),
            StackValue::BigInteger(bytes) => Some(BigInt::from_signed_bytes_le(bytes)),
            StackValue::ByteString(bytes) | StackValue::Buffer(bytes) if bytes.len() <= 32 => {
                Some(BigInt::from_signed_bytes_le(bytes))
            }
            _ => None,
        }
    }

    /// Converts to a neo-vm-rs stack value.
    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(vec![
            StackValue::ByteString(self.contract_hash.to_bytes()),
            StackValue::ByteString(self.method.as_bytes().to_vec()),
            StackValue::Integer(i64::from(self.arg_count)),
            StackValue::Integer(self.fixed_fee),
        ])
    }

    /// Updates this whitelisted contract from a neo-vm-rs stack value.
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<()> {
        let StackValue::Struct(items) = stack_value else {
            return Err(Error::invalid_format(
                "WhitelistedContract expects Struct stack value",
            ));
        };

        if items.len() < 4 {
            return Err(Error::invalid_format(format!(
                "WhitelistedContract stack value must contain 4 elements, found {}",
                items.len()
            )));
        }

        if let Some(bytes) = items[0].to_byte_string_bytes() {
            if let Ok(hash) = UInt160::from_bytes(&bytes) {
                self.contract_hash = hash;
            }
        }

        if let Some(bytes) = items[1].to_byte_string_bytes() {
            if let Ok(method) = String::from_utf8(bytes) {
                self.method = method;
            }
        }

        if let Some(count) = Self::stack_value_to_bigint(&items[2]).and_then(|value| value.to_u32())
        {
            self.arg_count = count;
        }

        if let Some(fee) = Self::stack_value_to_bigint(&items[3]).and_then(|value| value.to_i64()) {
            self.fixed_fee = fee;
        }

        Ok(())
    }
}

impl PolicyContract {
    fn serialize_whitelisted_contract(whitelisted: &WhitelistedContract) -> Result<Vec<u8>> {
        BinarySerializer::serialize_stack_value(
            &whitelisted.to_stack_value(),
            &ExecutionEngineLimits::default(),
        )
        .map_err(|e| Error::native_contract(format!("Failed to serialize whitelist info: {e}")))
    }

    fn deserialize_whitelisted_contract(bytes: &[u8]) -> Result<WhitelistedContract> {
        let stack_value = BinarySerializer::deserialize_stack_value(bytes).map_err(|e| {
            Error::native_contract(format!("Failed to deserialize whitelist info: {e}"))
        })?;

        let mut whitelist = WhitelistedContract::default();
        whitelist.from_stack_value(stack_value).map_err(|e| {
            Error::native_contract(format!("Failed to deserialize WhitelistedContract: {e}"))
        })?;
        Ok(whitelist)
    }
}

/// The Policy native contract.
pub struct PolicyContract {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl PolicyContract {
    const ID: i32 = -7;
    const CPU_FEE: i64 = 1 << 15;

    /// The default execution fee factor.
    pub const DEFAULT_EXEC_FEE_FACTOR: u32 = 30;

    /// The default storage price.
    pub const DEFAULT_STORAGE_PRICE: u32 = 100_000;

    /// The default network fee per byte of transactions.
    /// In the unit of datoshi, 1 datoshi = 1e-8 GAS.
    pub const DEFAULT_FEE_PER_BYTE: u32 = 1_000;

    /// The default fee for attributes.
    pub const DEFAULT_ATTRIBUTE_FEE: u32 = 0;

    /// The default fee for `NotaryAssisted` attribute after Echidna.
    pub const DEFAULT_NOTARY_ASSISTED_ATTRIBUTE_FEE: u32 = 10_000_000;

    /// Maximum execution fee factor committee can set.
    pub const MAX_EXEC_FEE_FACTOR: u32 = 100;

    /// Maximum attribute fee committee can set.
    pub const MAX_ATTRIBUTE_FEE: u32 = 10_0000_0000;

    /// Maximum storage price committee can set.
    pub const MAX_STORAGE_PRICE: u32 = 10_000_000;

    /// Maximum block generation time committee can set in milliseconds.
    pub const MAX_MILLISECONDS_PER_BLOCK: u32 = 30_000;

    /// Maximum MaxValidUntilBlockIncrement committee can set.
    pub const MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 = 86_400;

    /// Maximum MaxTraceableBlocks committee can set.
    pub const MAX_MAX_TRACEABLE_BLOCKS: u32 = 2_102_400;

    /// Required waiting time before recoverFund can execute (milliseconds).
    const REQUIRED_TIME_FOR_RECOVER_FUND_MS: u64 = 365 * 24 * 60 * 60 * 1_000;

    // Whitelist fee contracts prefix
    const PREFIX_WHITELISTED_FEE_CONTRACTS: u8 = 16;

    const PREFIX_BLOCKED_ACCOUNT: u8 = 15;
    const PREFIX_FEE_PER_BYTE: u8 = 10;
    const PREFIX_EXEC_FEE_FACTOR: u8 = 18;
    const PREFIX_STORAGE_PRICE: u8 = 19;
    const PREFIX_ATTRIBUTE_FEE: u8 = 20;
    const PREFIX_MILLISECONDS_PER_BLOCK: u8 = 21;
    const PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT: u8 = 22;
    const PREFIX_MAX_TRACEABLE_BLOCKS: u8 = 23;

    const MILLISECONDS_PER_BLOCK_CHANGED_EVENT_NAME: &'static str = "MillisecondsPerBlockChanged";

    /// Creates a new Policy contract.
    pub fn new() -> Self {
        // Policy contract hash: 0xcc5e4edd9f5f8dba8bb65734541df7a1c081c67b
        let hash = UInt160::parse("0xcc5e4edd9f5f8dba8bb65734541df7a1c081c67b")
            .expect("PolicyContract hash should be valid");

        let methods = vec![
            NativeMethod::safe(
                "getFeePerByte".to_string(),
                Self::CPU_FEE,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_required_call_flags(CallFlags::READ_STATES),
            NativeMethod::safe(
                "getExecFeeFactor".to_string(),
                Self::CPU_FEE,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_required_call_flags(CallFlags::READ_STATES),
            NativeMethod::safe(
                "getExecPicoFeeFactor".to_string(),
                Self::CPU_FEE,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_required_call_flags(CallFlags::READ_STATES),
            NativeMethod::safe(
                "getStoragePrice".to_string(),
                Self::CPU_FEE,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_required_call_flags(CallFlags::READ_STATES),
            NativeMethod::safe(
                "getMillisecondsPerBlock".to_string(),
                Self::CPU_FEE,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_required_call_flags(CallFlags::READ_STATES),
            NativeMethod::safe(
                "getMaxValidUntilBlockIncrement".to_string(),
                Self::CPU_FEE,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_required_call_flags(CallFlags::READ_STATES),
            NativeMethod::safe(
                "getMaxTraceableBlocks".to_string(),
                Self::CPU_FEE,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_required_call_flags(CallFlags::READ_STATES),
            // getAttributeFee overloads (hardfork switch at Echidna).
            NativeMethod::safe(
                "getAttributeFee".to_string(),
                Self::CPU_FEE,
                vec![ContractParameterType::Integer],
                ContractParameterType::Integer,
            )
            .with_deprecated_in(Hardfork::HfEchidna)
            .with_required_call_flags(CallFlags::READ_STATES)
            .with_parameter_names(vec!["attributeType".to_string()]),
            NativeMethod::safe(
                "getAttributeFee".to_string(),
                Self::CPU_FEE,
                vec![ContractParameterType::Integer],
                ContractParameterType::Integer,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_required_call_flags(CallFlags::READ_STATES)
            .with_parameter_names(vec!["attributeType".to_string()]),
            // Setters.
            NativeMethod::unsafe_method(
                "setFeePerByte".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_parameter_names(vec!["value".to_string()]),
            NativeMethod::unsafe_method(
                "setExecFeeFactor".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_parameter_names(vec!["value".to_string()]),
            NativeMethod::unsafe_method(
                "setStoragePrice".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_parameter_names(vec!["value".to_string()]),
            NativeMethod::unsafe_method(
                "setMillisecondsPerBlock".to_string(),
                Self::CPU_FEE,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(vec!["value".to_string()]),
            NativeMethod::unsafe_method(
                "setMaxValidUntilBlockIncrement".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(vec!["value".to_string()]),
            NativeMethod::unsafe_method(
                "setMaxTraceableBlocks".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(vec!["value".to_string()]),
            // setAttributeFee overloads (hardfork switch at Echidna).
            NativeMethod::unsafe_method(
                "setAttributeFee".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![
                    ContractParameterType::Integer,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Void,
            )
            .with_deprecated_in(Hardfork::HfEchidna)
            .with_parameter_names(vec!["attributeType".to_string(), "value".to_string()]),
            NativeMethod::unsafe_method(
                "setAttributeFee".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![
                    ContractParameterType::Integer,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(vec!["attributeType".to_string(), "value".to_string()]),
            // Account policy.
            NativeMethod::safe(
                "isBlocked".to_string(),
                Self::CPU_FEE,
                vec![ContractParameterType::Hash160],
                ContractParameterType::Boolean,
            )
            .with_required_call_flags(CallFlags::READ_STATES)
            .with_parameter_names(vec!["account".to_string()]),
            // BlockAccount overloads (hardfork switch at Faun)
            NativeMethod::unsafe_method(
                "blockAccount".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Hash160],
                ContractParameterType::Boolean,
            )
            .with_deprecated_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["account".to_string()]),
            NativeMethod::unsafe_method(
                "blockAccount".to_string(),
                Self::CPU_FEE,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
                vec![ContractParameterType::Hash160],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["account".to_string()]),
            NativeMethod::unsafe_method(
                "unblockAccount".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Hash160],
                ContractParameterType::Boolean,
            )
            .with_parameter_names(vec!["account".to_string()]),
            NativeMethod::safe(
                "getBlockedAccounts".to_string(),
                Self::CPU_FEE,
                Vec::new(),
                ContractParameterType::InteropInterface,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_required_call_flags(CallFlags::READ_STATES),
            // Whitelist management (HF_Faun)
            NativeMethod::unsafe_method(
                "setWhitelistFeeContract".to_string(),
                Self::CPU_FEE,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::String,
                    ContractParameterType::Integer,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec![
                "contractHash".to_string(),
                "method".to_string(),
                "argCount".to_string(),
                "fixedFee".to_string(),
            ]),
            NativeMethod::unsafe_method(
                "removeWhitelistFeeContract".to_string(),
                Self::CPU_FEE,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::String,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec![
                "contractHash".to_string(),
                "method".to_string(),
                "argCount".to_string(),
            ]),
            NativeMethod::safe(
                "getWhitelistFeeContracts".to_string(),
                Self::CPU_FEE,
                Vec::new(),
                ContractParameterType::InteropInterface,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_required_call_flags(CallFlags::READ_STATES),
            // Recover fund (HF_Faun)
            NativeMethod::unsafe_method(
                "recoverFund".to_string(),
                Self::CPU_FEE,
                CallFlags::ALL.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Hash160,
                ],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["account".to_string(), "token".to_string()]),
        ];

        Self {
            id: Self::ID,
            hash,
            methods,
        }
    }
}

// Include implementation files
mod account;
mod getters;
mod helpers;
mod native_impl;
mod setters;
mod snapshot;

#[cfg(test)]
mod tests;
